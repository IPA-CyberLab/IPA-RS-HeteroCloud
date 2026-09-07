#!/usr/bin/env python3
"""Positive DNS CLI checks using disposable container-local hosts fixtures."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--released", type=Path, required=True)
    parser.add_argument("--patched", type=Path, required=True)
    parser.add_argument("--image", default="ubuntu:24.04")
    parser.add_argument("--report", type=Path, default=Path("/tmp/heterocloud-dns-hosts-audit.json"))
    args = parser.parse_args()
    transcript, results = [], []
    container = "heterocloud-dns-audit-" + uuid.uuid4().hex[:12]

    def docker(*command, timeout=30, check=True):
        started = time.monotonic()
        result = subprocess.run(["docker", *command], capture_output=True, text=True, timeout=timeout)
        transcript.append(dict(command=["docker", *command], exit=result.returncode,
                               stdout=result.stdout, stderr=result.stderr,
                               elapsed_seconds=round(time.monotonic() - started, 3)))
        if check:
            result.check_returncode()
        return result

    report = {"boundary":"container-local /etc/hosts; no network; fake kubectl/helm",
              "results":results,"docker_transcript":transcript,"binaries":{}}
    created = False
    try:
        report["image_id"] = docker("image", "inspect", args.image, "--format", "{{.Id}}").stdout.strip()
        with tempfile.TemporaryDirectory(prefix="heterocloud-dns-hosts-") as directory:
            fixture = Path(directory)
            fixture.chmod(0o755)
            (fixture / "stubs").mkdir()
            for tool in ("kubectl", "helm"):
                path = fixture / "stubs" / tool
                path.write_text("#!/bin/sh\n/bin/cat >/dev/null\nprintf '%s\\n' \"$0 $*\" >> /tmp/stub-calls\nprintf 'fixture accepted\\n'\n")
                path.chmod(0o755)
            hosts = ["cloud-a.audit.invalid", "audit.invalid", "flow.audit.invalid",
                     "registry.audit.invalid", "s3.audit.invalid"]
            options = ["create", "--name", container, "--network", "none",
                       "--tmpfs", "/tmp", "--cap-drop", "ALL", "--security-opt", "no-new-privileges"]
            for host in hosts:
                options += ["--add-host", host + ":192.0.2.10"]
            docker(*options, "--entrypoint", "/bin/sleep", args.image, "300")
            created = True
            docker("cp", str(fixture) + "/.", container + ":/audit")
            for label, binary in (("released",args.released), ("patched",args.patched)):
                report["binaries"][label] = {"sha256":hashlib.sha256(binary.read_bytes()).hexdigest()}
                docker("cp", str(binary.resolve()), container + ":/audit/" + label)
            docker("start", container)
            report["hosts"] = docker("exec", container, "/bin/cat", "/etc/hosts").stdout
            for label in ("released", "patched"):
                version = docker("exec", container, "/audit/" + label, "--version")
                report["binaries"][label]["version"] = version.stdout.strip()
                for leaf in ("verify", "reconcile"):
                    command = ["exec", "--env", "PATH=/audit/stubs", "--env", "HOME=/tmp",
                               "--env", "KUBECONFIG=/tmp/nonexistent", container,
                               "/audit/" + label, "dns", leaf, "--domain", "audit.invalid",
                               "--public-ip", "192.0.2.10", "--allow-non-public"]
                    if leaf == "reconcile":
                        command += ["--provider", "cloudflare", "--timeout-seconds", "30"]
                    result = docker(*command, timeout=40, check=False)
                    expected = "Verified" if leaf == "verify" else "ExternalDNS converged all records"
                    passed = result.returncode == 0 and expected in result.stdout
                    results.append(dict(name=label + ":dns:" + leaf, passed=passed,
                                        **transcript[-1]))
                    print(f"{'PASS' if passed else 'FAIL'} {label} dns {leaf}: {result.stdout.strip()}", flush=True)
            report["stub_calls"] = docker("exec", container, "/bin/cat", "/tmp/stub-calls").stdout.splitlines()
            assert len(report["stub_calls"]) == 14, "expected seven fake cluster commands per reconcile"
    finally:
        if created:
            docker("rm", "--force", container, check=False)
        report.update(total=len(results), passed=sum(r["passed"] for r in results),
                      failed=sum(not r["passed"] for r in results))
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    return 1 if report["failed"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
