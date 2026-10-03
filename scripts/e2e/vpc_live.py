#!/usr/bin/env python3
"""Live VPC/Flash test against disposable fixture tenants (no mocked data plane).

Run vpc_fixture.py first. Credentials and database URL are read from private
files; reports contain only resource IDs, checks, and observed NAT addresses.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

IMAGE = "docker.io/library/python@sha256:2dd78ad5cf13a0b68f5134dc49aa9950203a8cf4b7463431b9f3b398287c5059"
HERE = Path(__file__).resolve().parent
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


class Api:
    def __init__(self, endpoint, tenant):
        self.tenant = tenant
        self.base = endpoint.rstrip("/") + "/api/v1/organizations/" + tenant["organization_id"]

    def call(self, method, path, body=None, expected=None):
        request = urllib.request.Request(self.base + "/" + path, method=method, data=None if body is None else json.dumps(body).encode(), headers={"Authorization": "Bearer " + self.tenant["api_key"], "Content-Type": "application/json", "User-Agent": "HeteroCloud-VPC-E2E/1.0"})
        try:
            with opener.open(request, timeout=25) as response:
                status = response.status
                data = response.read()
        except urllib.error.HTTPError as error:
            status, data = error.code, error.read()
        if expected is not None:
            assert status in expected, f"{method} {path}: expected {expected}, got {status}"
        else:
            assert 200 <= status < 300, f"{method} {path}: HTTP {status}"
        return json.loads(data) if data else None

    def wait(self, path, deleted=False, seconds=600):
        deadline = time.monotonic() + seconds
        last = None
        while time.monotonic() < deadline:
            value = self.call("GET", path, expected=[200, 404])
            if deleted and value.get("error", {}).get("code") == "not_found":
                return
            if not deleted and value.get("state") == "ready":
                return value
            last = value.get("state", value.get("error"))
            time.sleep(3)
        raise AssertionError(f"{path}: readiness timed out ({last})")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--dsn-file", type=Path, help="Administrator fixture setup only; never sent to a container")
    parser.add_argument("--cli", default="heterocloud")
    parser.add_argument("--region", default="heteronet-global")
    parser.add_argument("--trace-url", help="Owned HTTPS endpoint returning an ip= line; defaults to endpoint /cdn-cgi/trace")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--cleanup", action="store_true")
    parser.add_argument("--keep-on-failure", action="store_true")
    args = parser.parse_args()
    fixture = json.loads(args.fixture.read_text())
    assert all(t["slug"].startswith("vpc-e2e-") for t in fixture["tenants"])
    apis = [Api(args.endpoint, t) for t in fixture["tenants"]]
    report = {"endpoint": args.endpoint, "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "checks": [], "resources": []}

    def record(name, **details):
        report["checks"].append({"name": name, "passed": True, **details})
        args.report.write_text(json.dumps(report, indent=2))
        print("PASS " + name, flush=True)

    def clean():
        for api in apis:
            for collection in ["flash/services", "vpc/networks"]:
                items = api.call("GET", collection)["items"]
                for item in items:
                    assert item["project_id"] == api.tenant["project_id"] and item["name"].startswith("vpc-e2e-"), "Refusing to delete a non-fixture service"
                    if item["state"] != "deleting":
                        api.call("DELETE", collection + "/" + item["id"])
                for item in items:
                    api.wait(collection + "/" + item["id"], deleted=True)
        record("all_test_workloads_and_networks_removed")

    if args.cleanup:
        clean()
        return
    keyfile = args.fixture.parent / "driver-api-key"
    if keyfile.exists():
        keyfile.unlink()
    fd = os.open(keyfile, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as stream:
        stream.write(apis[0].tenant["api_key"])
    env = {**os.environ, "HETEROCLOUD_ENDPOINT": args.endpoint, "HETEROCLOUD_ORGANIZATION_ID": apis[0].tenant["organization_id"], "HETEROCLOUD_API_KEY_FILE": str(keyfile)}

    def cli(*arguments, body=None):
        result = subprocess.run([args.cli, *arguments], input=None if body is None else json.dumps(body), text=True, capture_output=True, env=env, timeout=650)
        assert result.returncode == 0, f"CLI {' '.join(arguments[:2])} failed: {result.stderr[:500]}"
        return json.loads(result.stdout)

    def ensure(api, collection, name, spec, use_cli=False):
        existing = [x for x in api.call("GET", collection)["items"] if x["name"] == name]
        assert len(existing) <= 1
        kind = "vpc" if collection == "vpc/networks" else "flash"
        value = existing[0] if existing else (cli(kind, "create", "--no-wait", body={"project_id": api.tenant["project_id"], "name": name, "spec": spec}) if use_cli else api.call("POST", collection, {"project_id": api.tenant["project_id"], "name": name, "spec": spec}))
        report["resources"].append({"organization_id": api.tenant["organization_id"], "provider": value["provider"], "id": value["id"]})
        args.report.write_text(json.dumps(report, indent=2))
        return api.wait(collection + "/" + value["id"])

    def network_spec():
        return {"region": args.region, "security_groups": ["parents", "children", "other"], "rules": [], "nat": {"enabled": False}}

    def hostname(vpc, name):
        return f"{name}.hc-vpc-{vpc['id'].replace('-', '')}.svc.cluster.local"

    marker = fixture.setdefault("marker", secrets.token_hex(16))
    args.fixture.write_text(json.dumps(fixture))

    def flash(vpc, name, group, program="vpc_child.py", public=False, extra_env=None):
        ports = [{"name": "http", "protocol": "tcp", "container_port": 8080}]
        if not public:
            ports += [{"name": "udp", "protocol": "udp", "container_port": 8081}, {"name": "blocked", "protocol": "tcp", "container_port": 8082}]
        return {"region": args.region, "image": IMAGE, "replicas": 1, "cpu_millis": 100, "memory_mib": 128, "ephemeral_storage_gib": 1, "ports": ports, "exposure": {"type": "public" if public else "internal", "traffic_mode": "forwarded", "endpoint_mode": "web" if public else "ip"}, "network": {"vpc_id": vpc["id"], "security_groups": [group], "private_name": name}, "env": {"E2E_PROGRAM": (HERE / program).read_text(), "E2E_MARKER": marker, **(extra_env or {})}, "command": ["python3"], "args": ["-u", "-c", "import os;exec(os.environ['E2E_PROGRAM'])"], "metadata": {"purpose": "vpc-e2e"}}

    passed = False
    try:
        api, foreign = apis
        main_vpc = ensure(api, "vpc/networks", "vpc-e2e-main", network_spec(), True)
        other_vpc = ensure(api, "vpc/networks", "vpc-e2e-other", network_spec())
        foreign_vpc = ensure(foreign, "vpc/networks", "vpc-e2e-foreign", network_spec())
        assert not main_vpc["spec"]["nat"]["enabled"] and not main_vpc["spec"]["rules"]
        assert cli("vpc", "get", main_vpc["id"])["id"] == main_vpc["id"]
        assert len(cli("vpc", "list")) == 2
        record("api_and_cli_vpc_lifecycle_private_defaults")
        foreign.call("GET", "vpc/networks/" + main_vpc["id"], expected=[403, 404])
        foreign.call("POST", "flash/services", {"project_id": foreign.tenant["project_id"], "name": "vpc-e2e-cross-org", "spec": flash(main_vpc, "invalid", "children")}, expected=[400, 403, 404, 409])
        record("cross_organization_api_access_and_attachment_denied")
        ensure(api, "flash/services", "vpc-e2e-bystander", flash(main_vpc, "bystander", "other"), True)
        ensure(api, "flash/services", "vpc-e2e-other-vpc", flash(other_vpc, "other-vpc", "children"))
        ensure(foreign, "flash/services", "vpc-e2e-other-org", flash(foreign_vpc, "other-org", "children"))
        tenant = json.loads(args.fixture.read_text())["tenants"][0]
        if "parent_api_key" not in tenant:
            assert args.dsn_file, "Provide --dsn-file for administrator-only scoped parent fixture provisioning"
            subprocess.run([sys.executable, str(HERE / "vpc_fixture.py"), "grant-parent", "--dsn-file", str(args.dsn_file), "--fixture", str(args.fixture), "--vpc-id", main_vpc["id"]], check=True)
            tenant = json.loads(args.fixture.read_text())["tenants"][0]
        child_manifest = {"name": "vpc-e2e-child", "project_id": api.tenant["project_id"], "spec": flash(main_vpc, "child", "children")}
        parent_spec = flash(main_vpc, "parent", "parents", "vpc_parent.py", True, {
            "E2E_API_BASE": api.base, "E2E_TRACE_URL": args.trace_url or args.endpoint.rstrip("/") + "/cdn-cgi/trace", "E2E_CHILD_MANIFEST": json.dumps(child_manifest),
            "E2E_TARGETS": json.dumps({"child": hostname(main_vpc, "child"), "bystander": hostname(main_vpc, "bystander"), "other-vpc": hostname(other_vpc, "other-vpc"), "other-org": hostname(foreign_vpc, "other-org")}),
        })
        parent = ensure(api, "flash/services", "vpc-e2e-parent", parent_spec)
        api.call("PUT", f"flash/services/{parent['id']}/secrets/child-api-key", {"value": tenant["parent_api_key"]})
        parent_spec["secret_env"] = {"HETEROCLOUD_API_KEY": "child-api-key"}
        api.call("PUT", "flash/services/" + parent["id"], {"name": parent["name"], "spec": parent_spec})
        parent = api.wait("flash/services/" + parent["id"])
        status = parent["status"].get("status", parent["status"])
        public = status["endpoints"][0]["host"]
        parent_url = public.rstrip("/") if public.startswith("https://") else "https://" + public

        def probe(name):
            with opener.open(parent_url + "/" + marker + "/" + name, timeout=40) as response:
                return json.load(response)

        def expect_probe(name, allowed, seconds=75):
            deadline = time.monotonic() + seconds
            last = None
            while time.monotonic() < deadline:
                try:
                    last = probe(name)
                    if last.get("ok") == allowed and (allowed or last.get("reason") == "connection_blocked"):
                        return last
                except (urllib.error.URLError, TimeoutError):
                    pass
                time.sleep(3)
            raise AssertionError(f"Probe {name}: expected allowed={allowed}, last={last}")

        expect_probe("nat", False)
        record("nat_disabled_blocks_public_egress")
        spec = network_spec()
        spec["nat"]["enabled"] = True
        cli("vpc", "update", main_vpc["id"], body={"name": main_vpc["name"], "spec": spec})
        nat = expect_probe("nat", True)
        record("nat_enabled_allows_public_egress", source_ip=nat["source_ip"])
        parent_spec["egress"] = {"mode": "disabled"}
        api.call("PUT", "flash/services/" + parent["id"], {"name": parent["name"], "spec": parent_spec})
        api.wait("flash/services/" + parent["id"])
        expect_probe("nat", False)
        record("nat_respects_workload_egress_disabled")
        parent_spec["egress"] = {"mode": "internet"}
        api.call("PUT", "flash/services/" + parent["id"], {"name": parent["name"], "spec": parent_spec})
        api.wait("flash/services/" + parent["id"])
        expect_probe("nat", True)
        record("workload_egress_can_be_restored")
        child = expect_probe("create", True)
        child = api.wait("flash/services/" + child["child_id"])
        record("parent_creates_private_child_via_api_and_secret_key", child_id=child["id"])
        forbidden = probe("forbidden-group")
        assert forbidden.get("http_status") == 403, forbidden
        record("parent_cannot_attach_unauthorized_security_group")
        expect_probe("child-tcp", False)
        expect_probe("child-udp", False)
        record("private_traffic_default_deny_tcp_and_udp")
        spec["rules"] = [{"source": {"type": "security_group", "name": "parents"}, "destination": {"type": "security_group", "name": "children"}, "protocol": protocol, "port": port} for protocol, port in [("tcp", 8080), ("udp", 8081)]]
        cli("vpc", "update", main_vpc["id"], body={"name": main_vpc["name"], "spec": spec})
        tcp = expect_probe("child-tcp", True)
        assert not tcp["parent_key_present"]
        expect_probe("child-udp", True)
        record("private_dns_tcp_udp_and_no_child_secret_inheritance")
        for name in ["wrong-port", "other-group", "other-vpc", "other-org"]:
            expect_probe(name, False)
            record(name + "_denied")
        spec["rules"][0]["source"] = {"type": "service", "service_id": parent["id"]}
        spec["rules"][0]["destination"] = {"type": "service", "service_id": child["id"]}
        cli("vpc", "update", main_vpc["id"], body={"name": main_vpc["name"], "spec": spec})
        expect_probe("child-tcp", True)
        record("explicit_service_to_service_rule")
        private_status = child["status"].get("status", child["status"])
        assert private_status.get("private_endpoints"), "Private DNS missing from Flash status"
        public_host = urllib.parse.urlsplit(parent_url).hostname
        child_public = "https://f-" + child["id"] + "." + public_host.split(".", 1)[1]
        try:
            with opener.open(child_public, timeout=15) as response:
                body = response.read(8192).decode()
                assert marker not in body, "Private child became publicly reachable"
                raise AssertionError(f"Unexpected public child route: HTTP {response.status}")
        except urllib.error.HTTPError as error:
            assert error.code in (404, 421), f"Unexpected private-host response: {error.code}"
        record("private_child_has_no_public_route")
        api.call("DELETE", "vpc/networks/" + main_vpc["id"], expected=[400, 409])
        record("attached_vpc_deletion_refused")
        spec["rules"] = []
        spec["nat"]["enabled"] = False
        cli("vpc", "update", main_vpc["id"], body={"name": main_vpc["name"], "spec": spec})
        expect_probe("child-tcp", False)
        expect_probe("child-udp", False)
        expect_probe("nat", False)
        record("rule_and_nat_revocation_blocks_new_connections")
        own_services = api.call("GET", "flash/services")["items"]
        for item in own_services:
            assert item["project_id"] == api.tenant["project_id"] and item["name"].startswith("vpc-e2e-")
            api.call("DELETE", "flash/services/" + item["id"])
        for item in own_services:
            api.wait("flash/services/" + item["id"], deleted=True)
        cli("vpc", "delete", main_vpc["id"], "--yes")
        record("cli_deletes_detached_vpc_and_waits_for_cleanup")
        passed = True
    finally:
        keyfile.unlink(missing_ok=True)
        try:
            if passed or not args.keep_on_failure:
                clean()
        except Exception:
            passed = False
            report["cleanup_failed"] = True
            raise
        finally:
            report["passed"] = passed
            report["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            args.report.write_text(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
