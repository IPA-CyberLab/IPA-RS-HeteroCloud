#!/usr/bin/env python3
"""Exercise real code-server recovery after ENOSPC on an isolated Longhorn home.

Only the randomly named fixture Pod/PVC are filled, signaled and removed. The
reference workload is read to reuse its exact image and pull-secret references.
No user workspace processes or files are changed.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kubeconfig", required=True)
    parser.add_argument("--kubectl", default="kubectl")
    parser.add_argument("--namespace", default="heterocloud-flash-workloads")
    parser.add_argument("--reference-pod", required=True)
    parser.add_argument("--node", required=True)
    parser.add_argument("--storage-class", default="longhorn-static")
    parser.add_argument("--legacy-supervisor", type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--resume", action="store_true", help="Resume an untested fixture from its UID receipt after an API outage")
    parser.add_argument("--preflight-only", action="store_true", help="Check node disk headroom and cache without creating resources")
    args = parser.parse_args()
    prefix = "coder-disk-e2e-" + uuid.uuid4().hex[:10]
    kube = [args.kubectl, "--kubeconfig=" + args.kubeconfig, "--request-timeout=20s", "-n", args.namespace]
    report = {"fixture": prefix, "checks": [], "cleanup": False}
    created = {}
    if args.resume:
        report = json.loads(args.report.read_text())
        assert not report["cleanup"] and not report["checks"], "Only an untested fixture can be resumed"
        prefix = report["fixture"]
        assert prefix.startswith("coder-disk-e2e-") and len(prefix) == 25
        created = report["created"]

    def run(command, body=None, timeout=35):
        attempts = 3 if command[0] in ("get", "patch", "delete") else 1
        for attempt in range(attempts):
            try:
                result = subprocess.run(kube + command, input=body, capture_output=True, text=True, timeout=timeout)
                if result.returncode == 0:
                    return result.stdout
                if command[0] == "delete" and "NotFound" in result.stderr:
                    return ""
                message = result.stderr[-900:]
            except subprocess.TimeoutExpired:
                message = "Kubernetes API request timed out"
            if attempt + 1 < attempts:
                time.sleep(2)
        raise RuntimeError("Fixture operation failed: " + message)

    def get(kind, name):
        return json.loads(run(["get", kind, name, "-o", "json"]))

    def execute(script, timeout=35):
        return run(["exec", "-i", prefix, "-c", "ide", "--", "sh", "-s"], script, timeout)

    def record(check, **details):
        item = {"check": check, **details}
        report["checks"].append(item)
        print(json.dumps(item), flush=True)
        args.report.write_text(json.dumps(report, indent=2))

    def healthy():
        deadline = time.monotonic() + 100
        while time.monotonic() < deadline:
            if execute("curl -sS --max-time 3 -o /dev/null -w '%{http_code}' http://127.0.0.1:13337/healthz || true").strip() == "200":
                return
            time.sleep(2)
        raise RuntimeError("Actual code-server did not recover its HTTP endpoint")

    def install(path, content):
        encoded = base64.b64encode(content).decode()
        execute(f"printf %s {shlex.quote(encoded)} | base64 -d > {shlex.quote(path)}\nchmod 700 {shlex.quote(path)}\n")

    def start(path):
        execute(f"nohup sh {shlex.quote(path)} >/tmp/supervisor-launch.log 2>&1 </dev/null &\necho $! >/tmp/supervisor.pid\n")

    def alive():
        return execute("ps -o stat= -p \"$(cat /tmp/supervisor.pid)\" 2>/dev/null || true").strip().startswith(("S", "R", "D"))

    def fill():
        before = execute("df -B1 --output=size,avail /root | tail -n1").split()
        # gVisor does not implement fallocate on this NFS mount. Allocate the
        # disposable volume through its own share-manager's local ext4 mount,
        # then verify ENOSPC through the gVisor client's real mount as well.
        pvc = get("pvc", prefix)
        volume = pvc["spec"]["volumeName"]
        assert pvc["metadata"]["uid"] == created["pvc"]
        assert volume == "pvc-" + created["pvc"]
        assert get("pv", volume)["spec"]["claimRef"]["uid"] == created["pvc"]
        backend = "/export/" + volume
        script = f'''set -eu
path={shlex.quote(backend)}
grep -q " $path " /proc/mounts
available={int(before[1])}
if command -v fallocate >/dev/null 2>&1; then
  fallocate -l "$available" "$path/fixture-fill" || true
fi
dd if=/dev/zero of="$path/fixture-fill" bs=65536 oflag=append conv=notrunc 2>/tmp/fill-error || true
grep -q 'No space left' /tmp/fill-error
'''
        run(["-n", "longhorn-system", "exec", "-i", "share-manager-" + volume, "--", "sh", "-s"], script, timeout=100)
        # A final-block allocation can be reclaimed when the backend dd closes.
        # Consume those last blocks through NFS and flush them before checking
        # the error; a buffered 4 KiB write alone need not report ENOSPC yet.
        execute("""attempt=0
while [ "$attempt" -lt 16 ]; do
  dd if=/dev/zero of=/root/fixture-enospc-probe bs=65536 count=16 oflag=append conv=notrunc,fsync 2>/tmp/client-fill-error || true
  if grep -qi 'No space left\\|ENOSPC' /tmp/client-fill-error; then exit 0; fi
  attempt=$((attempt + 1))
done
cat /tmp/client-fill-error >&2
df -h /root >&2
exit 1
""", timeout=100)
        record("persistent_home_really_full", enospc=True, filesystem_size_bytes=int(before[0]))

    def stop_child():
        # Every node process in this fresh, private fixture belongs to the IDE.
        execute("pkill -TERM -x MainThread || true\n")

    reference = get("pod", args.reference_pod)
    node = get("node", args.node)
    assert node["metadata"].get("labels", {}).get("heteronetwork.io/control-plane-only") != "true"
    assert any(condition["type"] == "Ready" and condition["status"] == "True" for condition in node["status"]["conditions"])
    assert not any(condition["type"] in ("DiskPressure", "MemoryPressure", "PIDPressure") and condition["status"] != "False" for condition in node["status"]["conditions"]), "Destination node is under resource pressure"
    image = next(c["image"] for c in reference["spec"]["containers"] if c["name"] == "workload")
    assert "@sha256:" in image, "The native E2E requires an immutable workspace image"
    summary = json.loads(run(["get", "--raw", "/api/v1/nodes/" + args.node + "/proxy/stats/summary"]))["node"]
    filesystems = [("nodefs", summary.get("fs")), ("imagefs", summary.get("runtime", {}).get("imageFs"))]
    for name, filesystem in filesystems:
        assert filesystem and filesystem.get("capacityBytes") and filesystem.get("availableBytes") is not None, "Cannot verify " + name + " headroom; no fixture will be created"
        assert filesystem["availableBytes"] >= max(10 * 1024 ** 3, filesystem["capacityBytes"] // 4), name + " must have at least 25% and 10 GiB free; no fixture will be created"
    report["preflight"] = {name: {key: filesystem[key] for key in ["capacityBytes", "availableBytes"]} for name, filesystem in filesystems}
    assert any(image in entry["names"] for entry in node["status"].get("images", [])), "Pre-cache the image on a node with ample free disk before running the fixture"
    if args.preflight_only:
        print(json.dumps({"preflight_only": True, "node": args.node, "image_cached": True, "filesystems": report["preflight"], "resources_created": False}))
        return
    labels = {"app.kubernetes.io/part-of": "heterocloud-coder-disk-e2e", "heterocloud.io/fixture": prefix}
    claim = {"apiVersion": "v1", "kind": "PersistentVolumeClaim", "metadata": {"name": prefix, "labels": labels}, "spec": {"accessModes": ["ReadWriteMany"], "storageClassName": args.storage_class, "resources": {"requests": {"storage": "64Mi"}}}}
    pod = {"apiVersion": "v1", "kind": "Pod", "metadata": {"name": prefix, "labels": labels}, "spec": {
        "restartPolicy": "Never", "runtimeClassName": reference["spec"]["runtimeClassName"], "nodeSelector": {"kubernetes.io/hostname": args.node},
        "imagePullSecrets": reference["spec"].get("imagePullSecrets", []), "securityContext": reference["spec"].get("securityContext", {}),
        "containers": [{"name": "ide", "image": image, "imagePullPolicy": "Never", "env": [{"name": "HOME", "value": "/root"}], "command": ["/bin/sh", "-c"], "args": ["while :; do sleep 30; done"],
                        "resources": {"requests": {"cpu": "100m", "memory": "256Mi", "ephemeral-storage": "512Mi"}, "limits": {"cpu": "1", "memory": "768Mi", "ephemeral-storage": "1Gi"}}, "volumeMounts": [{"name": "home", "mountPath": "/root"}]}],
        "volumes": [{"name": "home", "persistentVolumeClaim": {"claimName": prefix}}], "automountServiceAccountToken": False,
    }}
    marker = uuid.uuid4().hex
    try:
        if not args.resume:
            for kind, obj in [("pvc", claim), ("pod", pod)]:
                result = json.loads(run(["create", "-f", "-", "-o", "json"], json.dumps(obj)))
                created[kind] = result["metadata"]["uid"]
                report["created"] = created
                args.report.write_text(json.dumps(report, indent=2))
        else:
            assert get("pvc", prefix)["metadata"]["uid"] == created["pvc"]
            assert get("pod", prefix)["metadata"]["uid"] == created["pod"]
        deadline = time.monotonic() + 240
        while time.monotonic() < deadline:
            current = get("pod", prefix)
            if current.get("status", {}).get("phase") == "Running":
                break
            if current.get("status", {}).get("phase") == "Failed":
                raise RuntimeError("Fixture Pod failed to start")
            time.sleep(3)
        else:
            raise RuntimeError("Native fixture did not start within four minutes")
        pvc = get("pvc", prefix)
        report.update({"pod_uid": created["pod"], "pvc_uid": created["pvc"], "image": image, "node": args.node, "runtime": pod["spec"]["runtimeClassName"]})
        execute("mkdir -p /root/projects /root/.cache/coder-code-server\nprintf %s " + shlex.quote(marker) + " >/root/persistent-marker\n")

        if args.legacy_supervisor:
            install("/tmp/legacy-supervisor.sh", args.legacy_supervisor.read_bytes())
            start("/tmp/legacy-supervisor.sh")
            healthy()
            fill()
            # A terminating IDE can free a last filesystem block. Force the
            # diagnostic write to return ENOSPC even in that timing window.
            execute("rm -f /root/.cache/coder-code-server/supervisor.log\nln -s /dev/full /root/.cache/coder-code-server/supervisor.log\n")
            stop_child()
            deadline = time.monotonic() + 25
            while alive() and time.monotonic() < deadline:
                time.sleep(1)
            assert not alive(), "Legacy supervisor did not reproduce the fatal ENOSPC failure"
            record("legacy_supervisor_dies_after_ide_exit_on_full_home")
            # The killed wrapper can leave its node worker alive briefly. All
            # code-server processes here belong exclusively to this fixture.
            execute("pkill -TERM -x MainThread || true\nrm /root/fixture-fill /root/fixture-enospc-probe\n")

        source = Path(__file__).resolve().parents[2] / "examples/coder/code-server-supervisor.sh"
        install("/tmp/fixed-supervisor.sh", source.read_bytes())
        start("/tmp/fixed-supervisor.sh")
        healthy()
        fill()
        stop_child()
        time.sleep(7)
        assert alive(), "Fixed supervisor exited while the persistent home was full"
        execute("grep -q 'code-server exited' /tmp/coder-code-server-0/supervisor.log\n")
        record("fixed_supervisor_survives_ide_failure_on_full_home")
        execute("rm /root/fixture-fill /root/fixture-enospc-probe\n")
        healthy()
        assert execute("cat /root/persistent-marker").strip() == marker
        assert get("pod", prefix)["metadata"]["uid"] == created["pod"]
        assert get("pod", prefix)["status"]["containerStatuses"][0]["restartCount"] == 0
        record("actual_ide_recovers_after_space_is_freed", http_status=200, marker_sha256=hashlib.sha256(marker.encode()).hexdigest(), pod_and_container_preserved=True)
    finally:
        # Only fixture resources with the recorded UIDs may be removed. Never
        # change the reclaim policy of an existing user volume.
        if "pvc" in created:
            pvc = get("pvc", prefix)
            assert pvc["metadata"]["uid"] == created["pvc"]
            if pvc["spec"].get("volumeName"):
                pv = get("pv", pvc["spec"]["volumeName"])
                assert pv["spec"]["claimRef"]["uid"] == created["pvc"]
                patch = [{"op": "test", "path": "/metadata/uid", "value": pv["metadata"]["uid"]}, {"op": "replace", "path": "/spec/persistentVolumeReclaimPolicy", "value": "Delete"}]
                run(["patch", "pv", pv["metadata"]["name"], "--type=json", "-p", json.dumps(patch)])
        for kind in ("pod", "pvc"):
            if kind in created:
                obj = get(kind, prefix)
                assert obj["metadata"]["uid"] == created[kind]
                run(["delete", kind, prefix, "--grace-period=5", "--wait=false"])
        for kind in ("pod", "pvc"):
            if kind in created:
                run(["wait", "--for=delete", kind + "/" + prefix, "--timeout=60s"], timeout=70)
        if "pvc" in created and pvc["spec"].get("volumeName"):
            run(["wait", "--for=delete", "pv/" + pvc["spec"]["volumeName"], "--timeout=90s"], timeout=100)
        report["cleanup"] = True
        args.report.write_text(json.dumps(report, indent=2))
        print(json.dumps({"cleanup": True, "fixture": prefix}), flush=True)


if __name__ == "__main__":
    main()
