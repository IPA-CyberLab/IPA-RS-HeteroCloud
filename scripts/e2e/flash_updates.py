#!/usr/bin/env python3
"""Live Flash replacement updates using an owned, disposable fixture.

Tests API/CLI/console updates, real PVC expansion and retained /root data.
Credentials stay in vpc_fixture.py's private file and child environment.
Kubernetes is used only to inspect this fixture's PVC/Pod allocation.
"""
import argparse
import asyncio
import json
import os
from pathlib import Path
import secrets
import subprocess
import time
import urllib.parse

from websockets.asyncio.client import connect
from vpc_live import Api

IMAGE = "ghcr.io/ipa-cyberlab/ipa-rs-heterocloud-flash@sha256:690a6f38cf643b7d8c3b309ae65fef3e8125a4a004d15ee332039bd25fa0300d"
GIB = 1024 ** 3


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--cli", required=True)
    parser.add_argument("--updated-image", required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--browser-output", type=Path)
    parser.add_argument("--kubectl", default="kubectl")
    parser.add_argument("--kubeconfig", type=Path, required=True)
    args = parser.parse_args()
    fixture = json.loads(args.fixture.read_text())
    tenant = fixture["tenants"][0]
    assert all(t["slug"].startswith("vpc-e2e-") for t in fixture["tenants"])
    api = Api(args.endpoint, tenant)
    other = Api(args.endpoint, fixture["tenants"][1])
    assert not args.report.exists(), "refusing to overwrite cleanup records"
    assert not api.call("GET", "flash/services")["items"], "fixture must be empty"
    nonce = secrets.token_hex(6)
    report = {"started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "checks": [], "resources": [], "passed": False}
    service = vpc = None
    kub = [args.kubectl, "--kubeconfig=" + str(args.kubeconfig), "--request-timeout=15s", "-n", "heterocloud-flash-workloads"]

    def save():
        args.report.write_text(json.dumps(report, indent=2) + "\n")
        args.report.chmod(0o600)

    def record(name, **details):
        report["checks"].append({"name": name, "passed": True, **details}); save()
        print("PASS " + name, flush=True)

    def kget(kind, name):
        return json.loads(subprocess.check_output(kub + ["get", kind, name, "-o", "json"]))

    async def probe(command):
        items = api.call("GET", path + "/containers")["items"]
        pod = next(x["name"] for x in items if x["ready"])
        marker = "CHECK_" + nonce + "_OK"
        url = args.endpoint.rstrip("/").replace("https:", "wss:").replace("http:", "ws:") + "/api/v1/organizations/" + tenant["organization_id"] + "/" + path + "/exec?" + urllib.parse.urlencode({"pod": pod})
        async with connect(url, additional_headers={"Authorization": "Bearer " + tenant["api_key"]}, origin=args.endpoint.rstrip("/"), proxy=None, open_timeout=20) as ws:
            # The complete sentinel never appears in terminal input/echo.
            await ws.send((command + " && printf '%s%s%s\\n' CHECK_ " + nonce + " _OK; exit\r").encode())
            data = b""
            async with asyncio.timeout(30):
                while marker.encode() not in data:
                    part = await ws.recv(); data += part.encode() if isinstance(part, str) else part
        return pod

    def ready():
        value = api.wait(path, seconds=240)
        inner = value["status"].get("status", value["status"])
        assert inner.get("ready_replicas") == value["spec"]["replicas"]
        assert not inner.get("storage_resize_pending", False)
        return value

    def update(spec, project=None, cli=False):
        body = {"name": service["name"], "spec": spec}
        if project: body["project_id"] = project
        if cli:
            env = {**os.environ, "HETEROCLOUD_ENDPOINT": args.endpoint, "HETEROCLOUD_ORGANIZATION_ID": tenant["organization_id"], "HETEROCLOUD_API_KEY": tenant["api_key"], "HETEROCLOUD_NO_UPDATE_CHECK": "1"}
            result = subprocess.run([args.cli, "flash", "update", service["id"], "--file", "-", "--no-wait"], input=json.dumps(body), env=env, text=True, capture_output=True, timeout=60)
            assert result.returncode == 0, "CLI update failed; output withheld"
            assert json.loads(result.stdout)["id"] == service["id"]
        else: api.call("PUT", path, body)
        return ready()

    def verify_disk(persistent_gib, rootfs_gib):
        pvc = kget("pvc", claim)
        assert pvc["metadata"]["uid"] == original_pvc["metadata"]["uid"]
        assert pvc["spec"]["volumeName"] == original_pvc["spec"]["volumeName"]
        inner = service["status"].get("status", service["status"])
        assert inner["persistent_storage_capacity_bytes"] >= persistent_gib * GIB
        assert inner["persistent_storage_bytes"] == persistent_gib * GIB
        if rootfs_gib is not None: assert inner["rootfs_storage_bytes"] == rootfs_gib * GIB
        asyncio.run(probe("test \"$(cat /root/flash-update-marker)\" = " + nonce))

    try:
        vpc = api.call("POST", "vpc/networks", {"project_id": tenant["project_id"], "name": "flash-updates-vpc-" + nonce, "spec": {"region": "heteronet-global", "nat": {"enabled": False}, "security_groups": ["default"], "rules": []}})
        report["resources"].append({"provider": "vpc", "id": vpc["id"]}); save(); api.wait("vpc/networks/" + vpc["id"])
        service = api.call("POST", "flash/services", {"project_id": tenant["project_id"], "name": "flash-updates-" + nonce, "spec": {
            "region": "heteronet-global", "image": IMAGE, "replicas": 1, "cpu_millis": 100, "memory_mib": 128,
            "ephemeral_storage_gib": 3, "rootfs_storage_gib": 1, "persistent_storage_gib": 1,
            "ports": [{"name": "echo", "protocol": "udp", "container_port": 7777}],
            "exposure": {"type": "internal", "traffic_mode": "forwarded"}, "egress": {"mode": "disabled"},
            "network": {"vpc_id": vpc["id"], "security_groups": ["default"], "private_name": "updates"},
            "env": {"HOME": "/root"}, "command": ["/usr/local/bin/flash-udp-echo"], "args": [], "metadata": {"purpose": "flash-updates-e2e"},
        }})
        path = "flash/services/" + service["id"]; claim = "flash-" + service["id"] + "-home"
        report["resources"].append({"provider": "flash", "id": service["id"]}); save(); service = ready()
        original_pvc = kget("pvc", claim)
        old_pod = asyncio.run(probe("printf '%s' " + nonce + " > /root/flash-update-marker"))
        record("created_with_explicit_rootfs_and_persistent_sizes", service_id=service["id"], pvc_uid=original_pvc["metadata"]["uid"])
        service = update({**service["spec"], "image": args.updated_image, "cpu_millis": 200, "memory_mib": 256,
            "ephemeral_storage_gib": 4, "rootfs_storage_gib": 2, "env": {"HOME": "/root", "UPDATED": "yes", "FLASH_ECHO_LISTEN": "0.0.0.0:7778"},
            "ports": [{"name": "echo", "protocol": "udp", "container_port": 7778}],
            "command": ["/bin/sh"], "args": ["-c", "exec /usr/local/bin/flash-udp-echo"], "metadata": {"purpose": "flash-updates-e2e", "updated": True}}, cli=True)
        verify_disk(1, 2)
        pod = asyncio.run(probe("test \"$UPDATED\" = yes")); assert pod != old_pod
        actual = kget("pod", pod)["spec"]["containers"][0]
        assert actual["resources"]["limits"]["ephemeral-storage"] == str(2 * GIB)
        assert actual["resources"]["limits"]["cpu"] == "200m"
        record("cli_updates_image_cpu_memory_rootfs_environment_command_args_metadata_and_ports")
        service = update({**service["spec"], "ephemeral_storage_gib": 5, "persistent_storage_gib": 2})
        verify_disk(2, 2)
        assert asyncio.run(probe("test -f /root/flash-update-marker")) == pod, "persistent expansion must not replace an unchanged container"
        asyncio.run(probe("test $(df -B1 --output=size /root | tail -n1) -gt 1600000000"))
        record("persistent_disk_and_filesystem_expanded_in_place_without_container_restart")
        service = update({**service["spec"], "exposure": {"type": "public", "traffic_mode": "forwarded"}})
        actual_service = kget("service", "flash-" + service["id"])
        assert actual_service["spec"]["type"] == "LoadBalancer"
        verify_disk(2, 2)
        service = update({**service["spec"], "exposure": {"type": "internal", "traffic_mode": "forwarded"}})
        actual_service = kget("service", "flash-" + service["id"])
        assert actual_service["spec"]["type"] == "ClusterIP"
        assert "loadBalancerClass" not in actual_service["spec"]
        verify_disk(2, 2)
        record("public_load_balancer_and_private_service_modes_can_be_changed_in_both_directions")
        if args.browser_output:
            subprocess.run(["node", str(Path(__file__).with_name("flash_updates_browser.mjs")), args.endpoint, str(args.fixture), service["id"], str(args.browser_output)], check=True, timeout=650)
            service = ready(); verify_disk(2, service["spec"]["rootfs_storage_gib"])
            record("real_desktop_and_mobile_console_edits_preserve_files", records=json.loads((args.browser_output/"browser-report.json").read_text())["records"])
        before = service["generation"]
        api.call("PUT", path, {"name": service["name"], "spec": {**service["spec"], "persistent_storage_gib": 1}}, expected=[400])
        api.call("PUT", path, {"name": service["name"], "project_id": fixture["tenants"][1]["project_id"], "spec": service["spec"]}, expected=[404])
        other.call("PUT", path, {"name": service["name"], "spec": service["spec"]}, expected=[403, 404])
        assert api.call("GET", path)["generation"] == before
        verify_disk(2, service["spec"]["rootfs_storage_gib"])
        record("shrink_and_cross_tenant_updates_rejected_without_changing_generation_or_data")
        project = api.call("POST", "projects", {"name": "Flash updates destination", "slug": "flash-updates-" + nonce})
        spec = {**service["spec"], "network": None, "ports": [], "egress": {"mode": "internet"}}
        service = update(spec, project=project["id"], cli=True)
        assert service["project_id"] == project["id"]
        verify_disk(2, service["spec"]["rootfs_storage_gib"])
        record("cli_moves_project_and_detaches_vpc_while_retaining_service_and_disk_identity")
        spec = dict(service["spec"]); spec.pop("rootfs_storage_gib", None)
        service = update(spec); verify_disk(2, None)
        record("explicit_rootfs_setting_can_be_removed_to_restore_automatic_allocation")
        report["passed"] = True
    except Exception as error:
        report["error_type"] = type(error).__name__; save(); raise
    finally:
        for provider, item in [("flash", service), ("vpc", vpc)]:
            if item:
                collection = "flash/services" if provider == "flash" else "vpc/networks"
                current = api.call("GET", collection + "/" + item["id"])
                assert current["organization_id"] == tenant["organization_id"] and current["name"].startswith("flash-updates-")
                api.call("DELETE", collection + "/" + item["id"]); api.wait(collection + "/" + item["id"], deleted=True)
        report["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()); save()
        print("Owned Flash update fixture resources removed", flush=True)


if __name__ == "__main__": main()
