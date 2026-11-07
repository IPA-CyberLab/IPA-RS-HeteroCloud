#!/usr/bin/env python3
"""Exercise real Flash logs using two disposable vpc_fixture.py tenants.

Creates one small stateless private workload and removes only that workload.
The fixture file is private; reports never contain API keys or workload logs.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

from vpc_live import Api


# This image contains Python, which the marker-emitting test workload needs.
IMAGE = "ghcr.io/ipa-cyberlab/ipa-rs-heterocloud-flash@sha256:69c4b4e5b0aefdb55f7f59c9ed7673476d974a7abdfd0b2d9429e4bb1de94d8f"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--region", default="heteronet-global")
    parser.add_argument("--cli", help="Optional CLI binary to verify against the same live workload")
    parser.add_argument("--browser-output", type=Path, help="Test desktop and mobile browser against the live service")
    args = parser.parse_args()
    assert not args.report.exists(), "refusing to overwrite cleanup evidence"
    fixture = json.loads(args.fixture.read_text())
    assert len(fixture["tenants"]) == 2
    tenant, other = fixture["tenants"]
    assert all(t["slug"].startswith("vpc-e2e-") for t in (tenant, other))
    assert tenant["organization_id"] != other["organization_id"]
    api = Api(args.endpoint, tenant)
    foreign = Api(args.endpoint, {**other, "organization_id": tenant["organization_id"]})
    nonce = secrets.token_hex(10)
    name, marker = "flash-logs-e2e-" + nonce, "flash-logs-e2e-marker-" + nonce
    report = {"schema_version": 1, "service_name": name, "checks": [], "passed": False}

    def save():
        args.report.write_text(json.dumps(report, indent=2) + "\n")
        args.report.chmod(0o600)

    def record(check):
        report["checks"].append(check)
        save()
        print("PASS " + check, flush=True)

    def cleanup():
        items = api.call("GET", "flash/services")["items"]
        matches = [item for item in items if item["name"] == name]
        assert len(matches) <= 1, "duplicate fixture name; refusing cleanup"
        for item in matches:
            assert item["organization_id"] == tenant["organization_id"]
            assert item["project_id"] == tenant["project_id"]
            if "service_id" in report:
                assert report["service_id"] == item["id"]
            else:
                report["service_id"] = item["id"]
                save()
            path = "flash/services/" + item["id"]
            if item["state"] != "deleting":
                api.call("DELETE", path)
            api.wait(path, deleted=True, seconds=300)
        record("only_disposable_workload_deleted")

    save()  # Keep the unique name even if creation returns an ambiguous error.
    try:
        assert all(item["name"] != name for item in api.call("GET", "flash/services")["items"])
        program = "import http.server; print(%r, flush=True); http.server.HTTPServer(('0.0.0.0',8080), http.server.BaseHTTPRequestHandler).serve_forever()" % marker
        service = api.call("POST", "flash/services", {
            "project_id": tenant["project_id"], "name": name,
            "spec": {
                "region": args.region, "image": IMAGE, "replicas": 1,
                "cpu_millis": 100, "memory_mib": 128, "ephemeral_storage_gib": 1,
                "deployment": {"stateless": True},
                "autoscaling": {"min_replicas": 1, "max_replicas": 1, "target_cpu_utilization_percent": 80},
                "ports": [{"name": "http", "protocol": "tcp", "container_port": 8080}],
                "exposure": {"type": "internal", "traffic_mode": "forwarded"},
                "egress": {"mode": "disabled"},
                "env": {}, "command": ["python3", "-u", "-c"], "args": [program],
                "metadata": {"test": "flash-logs-e2e"},
            },
        })
        service_id = service["id"]
        report["service_id"] = service_id
        save()
        path = "flash/services/" + service_id
        api.wait(path, seconds=300)
        record("stateless_private_workload_ready")

        endpoint = args.endpoint.rstrip("/") + "/api/v1/organizations/" + tenant["organization_id"] + "/" + path + "/logs"
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        try:
            request = urllib.request.Request(endpoint, headers={"User-Agent": "HeteroCloud-Logs-E2E/1.0"})
            opener.open(request, timeout=20)
            raise AssertionError("anonymous logs unexpectedly allowed")
        except urllib.error.HTTPError as error:
            assert error.code in (401, 403), "anonymous request was not rejected"
        foreign.call("GET", path + "/logs", expected=[403, 404])
        record("anonymous_and_other_organization_denied")

        pods = api.call("GET", path + "/logs")
        assert pods["text"] is None
        pod = next(item["name"] for item in pods["items"] if item["ready"])
        request = urllib.request.Request(endpoint + "?" + urllib.parse.urlencode({"pod": pod}),
            headers={"Authorization": "Bearer " + tenant["api_key"], "User-Agent": "HeteroCloud-Logs-E2E/1.0"})
        with opener.open(request, timeout=25) as response:
            assert "no-store" in response.headers.get("Cache-Control", "").lower()
            logs = json.load(response)
        assert logs["pod"] == pod and marker in logs["text"]
        record("real_pod_logs_and_no_store")
        api.call("GET", path + "/logs?" + urllib.parse.urlencode({"pod": "not-owned-" + nonce}), expected=[404])
        assert marker in api.call("GET", path + "/logs?" + urllib.parse.urlencode({"pod": pod}))["text"]
        record("foreign_pod_name_denied")

        if args.cli:
            with tempfile.TemporaryDirectory(prefix="hc-logs-cli-") as directory:
                key = Path(directory) / "key"
                key.write_text(tenant["api_key"])
                key.chmod(0o600)
                env = {**os.environ, "HETEROCLOUD_NO_UPDATE_CHECK": "1"}
                result = subprocess.run([args.cli, "--endpoint", args.endpoint,
                    "--organization-id", tenant["organization_id"], "--api-key-file", str(key),
                    "flash", "logs", service_id, "--pod", pod],
                    capture_output=True, text=True, timeout=40, env=env)
                assert result.returncode == 0, "CLI log request failed (output withheld)"
                assert marker in json.loads(result.stdout)["text"]
            record("real_cli_logs")
        if args.browser_output:
            subprocess.run(["node", str(Path(__file__).with_name("flash_logs_browser.mjs")),
                args.endpoint, str(args.fixture), service_id, marker, str(args.browser_output)],
                check=True, timeout=180)
            browser = json.loads((args.browser_output / "browser-report.json").read_text())
            assert browser["passed"] and len(browser["records"]) == 2
            record("real_desktop_and_mobile_logs_ui")
        report["passed"] = True
    except Exception as error:
        report["error_type"] = type(error).__name__
        raise
    finally:
        try:
            cleanup()
        except Exception:
            report["passed"] = False
            report["cleanup_failed"] = True
            raise
        finally:
            report["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            save()
    print("PASS live Flash logs E2E", flush=True)


if __name__ == "__main__":
    main()
