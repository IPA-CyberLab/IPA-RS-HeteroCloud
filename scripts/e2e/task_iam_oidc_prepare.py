#!/usr/bin/env python3
"""Prepare/remove owned live IAM/OIDC fixtures; never logs credentials.

Use a fresh vpc_fixture.py tenant. Identity-provider credentials stay in the
private fixture. The resulting services use the real API, VPC and Flash runtime.
"""
import argparse
import json
from pathlib import Path
import secrets
from vpc_live import Api

WEB_IMAGE = "docker.io/library/python@sha256:2dd78ad5cf13a0b68f5134dc49aa9950203a8cf4b7463431b9f3b398287c5059"
WEB_PROGRAM = """from http.server import BaseHTTPRequestHandler, HTTPServer
class Handler(BaseHTTPRequestHandler):
 def do_GET(self):
  body=b'<html><body><h1>HeteroCloud OIDC E2E</h1></body></html>'
  self.send_response(200)
  self.send_header('Content-Type','text/html; charset=utf-8')
  self.send_header('Content-Length',str(len(body)))
  self.end_headers()
  self.wfile.write(body)
 def log_message(self,*args): pass
HTTPServer(('0.0.0.0',8080),Handler).serve_forever()
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--parent-image", required=True, help="Immutable owned Debian-based image containing /bin/sleep and CA certificates")
    parser.add_argument("--oidc-realm", help="Existing publicly routed realm; only owned clients and users are created in it")
    parser.add_argument("--cleanup", action="store_true")
    args = parser.parse_args()
    fixture = json.loads(args.fixture.read_text())
    tenant = fixture["tenants"][0]
    assert tenant["slug"].startswith("vpc-e2e-")
    assert args.fixture.stat().st_mode & 0o077 == 0
    api = Api(args.endpoint, tenant)

    def save():
        args.fixture.write_text(json.dumps(fixture))

    if args.cleanup:
        for collection in ["flash/services", "vpc/networks"]:
            for item in api.call("GET", collection)["items"]:
                assert item["name"].startswith("task-iam-e2e-")
                assert item["project_id"] == tenant["project_id"]
                path = collection + "/" + item["id"]
                if item["state"] != "deleting":
                    if collection == "flash/services":
                        if item["spec"].get("exposure", {}).get("authentication"):
                            spec = {**item["spec"], "exposure": {**item["spec"]["exposure"], "authentication": None}}
                            api.call("PUT", path, {"name": item["name"], "spec": spec})
                            api.wait(path)
                        for secret in api.call("GET", path + "/secrets")["items"]:
                            api.call("DELETE", path + "/secrets/" + secret)
                    api.call("DELETE", path)
                api.wait(path, deleted=True)
        print("PASS disposable Flash services and VPC removed", flush=True)
        return
    if not fixture.get("task_iam"):
        assert not api.call("GET", "flash/services")["items"]
        fixture["task_iam"] = {"endpoint": args.endpoint, "nonce": secrets.token_hex(6)}
    save()
    test = fixture["task_iam"]
    assert test["endpoint"] == args.endpoint
    nonce = test["nonce"]
    if not test.get("principal_id"):
        role = api.call("POST", "iam/principals", {"name": "task-iam-e2e-controller-" + nonce})
        test["principal_id"] = role["id"]
        save()
    role = {"id": test["principal_id"]}
    if not test.get("vpc_id"):
        vpc = api.call("POST", "vpc/networks", {
        "project_id": tenant["project_id"], "name": "task-iam-e2e-vpc-" + nonce,
        "spec": {"region": "heteronet-global", "nat": {"enabled": True},
                 "security_groups": ["coder", "workspaces"], "rules": []},
        })
        test["vpc_id"] = vpc["id"]
        save()
    vpc = {"id": test["vpc_id"]}
    api.wait("vpc/networks/" + vpc["id"])
    org = tenant["organization_id"]
    if not test.get("policy_id"):
        policy = api.call("POST", "iam/policies", {
        "name": "task-iam-e2e-policy-" + nonce,
        "document": {"version": "2026-07-31", "statements": [
            {"effect": "Allow", "actions": ["flash:CreateInstance", "flash:ListInstances", "flash:GetInstance", "flash:DeleteInstance"], "resources": [f"hc:org:{org}:flash/*"]},
            {"effect": "Allow", "actions": ["vpc:AttachSecurityGroup"], "resources": [f"hc:org:{org}:vpc/network/{vpc['id']}/security-group/workspaces"]},
        ]},
        })
        test["policy_id"] = policy["id"]
        save()
    if not test.get("binding_id"):
        binding = api.call("POST", "iam/bindings", {"principal_id": role["id"], "policy_id": test["policy_id"]})
        test["binding_id"] = binding["id"]
        save()
    common = {"region": "heteronet-global", "replicas": 1, "cpu_millis": 100,
              "memory_mib": 128, "ephemeral_storage_gib": 1, "ports": [],
              "exposure": {"type": "internal", "traffic_mode": "forwarded"},
              "env": {}, "metadata": {}, "command": [], "args": []}
    if not test.get("parent_id"):
        parent = api.call("POST", "flash/services", {
        "project_id": tenant["project_id"], "name": "task-iam-e2e-parent-" + nonce,
        "spec": {**common, "image": args.parent_image, "cpu_millis": 1000,
                 "memory_mib": 256, "ephemeral_storage_gib": 3, "task_role": role["id"],
                 "network": {"vpc_id": vpc["id"], "security_groups": ["coder"], "private_name": "iam-parent"},
                 "command": ["/bin/sleep"], "args": ["infinity"]},
        })
        test["parent_id"] = parent["id"]
        save()
    if not test.get("web_id"):
        web = api.call("POST", "flash/services", {
        "project_id": tenant["project_id"], "name": "task-iam-e2e-web-" + nonce,
        "spec": {**common, "image": WEB_IMAGE,
                 "command": ["python3"], "args": ["-u", "-c", WEB_PROGRAM],
                 "ports": [{"name": "http", "protocol": "tcp", "container_port": 8080}],
                 "exposure": {"type": "public", "traffic_mode": "forwarded", "endpoint_mode": "web"}},
        })
        test["web_id"] = web["id"]
        save()
    api.wait("flash/services/" + test["parent_id"])
    ready = api.wait("flash/services/" + test["web_id"])
    status = ready["status"].get("status", ready["status"])
    test["callback_url"] = status["oidc_callback_url"]
    test["web_url"] = test["callback_url"].split("/_heterocloud/")[0]
    realm = args.oidc_realm or "hc-oidc-e2e-" + nonce
    fixture.setdefault("oidc_test", {
        "realm": realm, "client_id": "hc-oidc-e2e-" + nonce,
        "shared_realm": bool(args.oidc_realm), "nonce": nonce,
        "username": "hc-oidc-e2e-user-" + nonce, "password": secrets.token_urlsafe(32),
        "client_secret": secrets.token_urlsafe(32), "callback_url": test["callback_url"],
        "issuer_url": args.endpoint.rstrip("/") + "/id/realms/" + realm,
    })
    save()
    print("PASS real task-role parent with VPC NAT and anonymous web service ready", flush=True)


if __name__ == "__main__":
    main()
