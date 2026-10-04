#!/usr/bin/env python3
"""Exercise the documented public VPC example with an isolated fixture tenant."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request

from vpc_live import Api, opener


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--cli", default="heterocloud")
    parser.add_argument("--region", required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    tenant = json.loads(args.fixture.read_text())["tenants"][0]
    assert tenant["slug"].startswith("vpc-e2e-")
    api = Api(args.endpoint, tenant)
    assert not api.call("GET", "flash/services")["items"]
    assert not api.call("GET", "vpc/networks")["items"]
    report = {"started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "passed": False, "checks": [], "resources": []}
    root = Path(__file__).resolve().parents[2]
    keyfile = args.fixture.parent / "example-api-key"
    fd = os.open(keyfile, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as stream:
        stream.write(tenant["api_key"])
    env = {**os.environ, "HETEROCLOUD_ENDPOINT": args.endpoint, "HETEROCLOUD_ORGANIZATION_ID": tenant["organization_id"], "HETEROCLOUD_API_KEY_FILE": str(keyfile)}

    def record(name):
        report["checks"].append({"name": name, "passed": True})
        args.report.write_text(json.dumps(report, indent=2))
        print("PASS " + name, flush=True)

    def cli(*arguments, body=None):
        result = subprocess.run([args.cli, *arguments], input=None if body is None else json.dumps(body), text=True, capture_output=True, env=env, timeout=650)
        assert result.returncode == 0, f"CLI {' '.join(arguments[:2])} failed: {result.stderr[:500]}"
        return json.loads(result.stdout) if result.stdout.strip() else None

    def expect_https(url, public):
        deadline = time.monotonic() + 75
        last = None
        while time.monotonic() < deadline:
            request = urllib.request.Request(url, headers={"Cache-Control": "no-cache", "Connection": "close", "User-Agent": "HeteroCloud-VPC-E2E/1.0"})
            try:
                with opener.open(request, timeout=15) as response:
                    last = response.status
                    if public and response.status == 200 and b"Welcome to nginx!" in response.read(8192):
                        return
            except urllib.error.HTTPError as error:
                last = error.code
                if not public and error.code in (404, 421):
                    return
            except (urllib.error.URLError, TimeoutError) as error:
                last = type(error).__name__
            time.sleep(3)
        raise AssertionError(f"Returned HTTPS endpoint: expected public={public}, last={last}")

    def cleanup():
        for collection in ("flash/services", "vpc/networks"):
            items = api.call("GET", collection)["items"]
            for item in items:
                assert item["project_id"] == tenant["project_id"] and item["name"].startswith("vpc-e2e-example-")
                if item["state"] != "deleting":
                    api.call("DELETE", collection + "/" + item["id"])
            for item in items:
                api.wait(collection + "/" + item["id"], deleted=True)
        record("public_example_resources_removed")

    try:
        network = json.loads((root / "examples/cli/vpc.json").read_text())
        network.update(project_id=tenant["project_id"], name="vpc-e2e-example-network")
        network["spec"]["region"] = args.region
        vpc = cli("vpc", "create", body=network)
        report["resources"].append({"provider": "vpc", "id": vpc["id"]})
        manifest = json.loads((root / "examples/cli/flash-vpc-public.json").read_text())
        manifest.update(project_id=tenant["project_id"], name="vpc-e2e-example-public")
        manifest["spec"]["region"] = args.region
        manifest["spec"]["network"]["vpc_id"] = vpc["id"]
        service = cli("flash", "create", body=manifest)
        report["resources"].append({"provider": "flash", "id": service["id"]})
        assert not vpc["spec"]["nat"]["enabled"]
        assert service["spec"]["egress"]["mode"] == "disabled"
        record("documented_public_vpc_example_created_through_cli")
        status = service["status"].get("status", service["status"])
        assert len(status["endpoints"]) == 1
        endpoint = status["endpoints"][0]
        assert endpoint["protocol"] == "tcp" and endpoint["port"] == 443
        url = endpoint["host"].rstrip("/") if endpoint["host"].startswith("https://") else "https://" + endpoint["host"]
        parsed = urllib.parse.urlsplit(url)
        assert parsed.hostname.startswith("f-" + service["id"] + ".")
        assert parsed.scheme == "https" and parsed.port in (None, 443) and not parsed.query and not parsed.fragment
        private = status["private_endpoints"]
        assert private
        expect_https(url, True)
        record("api_returned_https_endpoint_works_with_nat_and_egress_disabled")
        spec = service["spec"]
        spec["exposure"] = {"type": "internal", "traffic_mode": "forwarded", "endpoint_mode": "ip"}
        service = cli("flash", "update", service["id"], body={"name": service["name"], "spec": spec})
        expect_https(url, False)
        status = service["status"].get("status", service["status"])
        assert status["private_endpoints"] == private
        record("returned_public_url_is_withdrawn_and_private_endpoints_are_preserved")
        report["passed"] = True
    finally:
        try:
            cleanup()
        except Exception:
            report["passed"] = False
            report["cleanup_failed"] = True
            raise
        finally:
            keyfile.unlink(missing_ok=True)
            report["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            args.report.write_text(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
