#!/usr/bin/env python3
"""Verify real Pod credentials and parent/child creation in an owned test tenant.

The parent receives no API key or personal login. Its CLI uses the projected
identity and token exchange. Credentials are held only in memory, never logged.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from vpc_live import Api


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--namespace", default="heterocloud-flash-workloads")
    args = parser.parse_args()
    assert args.fixture.stat().st_mode & 0o077 == 0
    fixture = json.loads(args.fixture.read_text())
    tenant, foreign = fixture["tenants"]
    test = fixture["task_iam"]
    assert tenant["slug"].startswith("vpc-e2e-")
    api = Api(test["endpoint"], tenant)
    report = {"schema_version": 1, "passed": False, "checks": [],
              "principal_id": test["principal_id"], "parent_id": test["parent_id"]}

    def record(name, **details):
        report["checks"].append({"name": name, "passed": True, **details})
        args.report.write_text(json.dumps(report, indent=2))
        print("PASS " + name, flush=True)

    def kube(*command, data=None):
        result = subprocess.run(["kubectl", "-n", args.namespace, *command],
                                input=data, capture_output=True, timeout=90)
        assert result.returncode == 0, "owned workload operation failed"
        return result.stdout

    pods = json.loads(kube("get", "pods", "-l", "flash.heterocloud.io/instance=" + test["parent_id"], "-o", "json"))["items"]
    pods = [pod for pod in pods if pod["status"]["phase"] == "Running" and not pod["metadata"].get("deletionTimestamp")]
    assert len(pods) == 1
    pod = pods[0]
    name = pod["metadata"]["name"]
    assert pod["metadata"]["annotations"]["iam.heterocloud.io/task-role"] == test["principal_id"]
    assert pod["spec"]["serviceAccountName"] == "flash-" + test["parent_id"].replace("-", "")
    assert pod["spec"]["automountServiceAccountToken"] is False
    volume = next(v for v in pod["spec"]["volumes"] if v["name"] == "heterocloud-workload-identity")
    projection = volume["projected"]["sources"][0]["serviceAccountToken"]
    assert projection["audience"] == "heterocloud-workload" and projection["expirationSeconds"] == 3600
    record("pod_bound_short_lived_identity_without_default_kubernetes_token")
    kube("exec", "-i", name, "-c", "workload", "--", "sh", "-c",
         "umask 077; mkdir -p /root/.local/bin; cat > /root/.local/bin/heterocloud; chmod 700 /root/.local/bin/heterocloud",
         data=args.cli.read_bytes())
    kube("exec", name, "-c", "workload", "--", "sh", "-c",
         'test -z "${HETEROCLOUD_API_KEY:-}" && test -z "${HETEROCLOUD_API_KEY_FILE:-}" && test -r "$HETEROCLOUD_WORKLOAD_TOKEN_FILE"')

    def cli(*command, body=None, organization=None, denied=False):
        invocation = ["kubectl", "-n", args.namespace, "exec", "-i", name, "-c", "workload", "--"]
        if organization:
            invocation += ["env", "HETEROCLOUD_ORGANIZATION_ID=" + organization]
        invocation += ["/root/.local/bin/heterocloud", *command]
        result = subprocess.run(invocation, input=None if body is None else json.dumps(body).encode(),
                                capture_output=True, timeout=100)
        if denied:
            assert result.returncode != 0, "forbidden workload operation succeeded"
            return
        assert result.returncode == 0, "automatic workload CLI authentication or operation failed"
        return json.loads(result.stdout)

    identity = cli("iam", "whoami")
    assert identity["type"] == "workload"
    assert identity["organization_id"] == tenant["organization_id"]
    assert identity["principal_id"] == test["principal_id"]
    assert identity["service_instance_id"] == test["parent_id"]
    assert identity["pod_uid"] == pod["metadata"]["uid"]
    record("container_cli_authenticates_without_key_or_personal_login")
    cli("iam", "service-accounts", "list", denied=True)
    cli("flash", "list", organization=foreign["organization_id"], denied=True)
    record("ungranted_iam_actions_and_cross_organization_access_denied")

    manifest = {
        "project_id": tenant["project_id"], "name": "task-iam-e2e-child-" + test["nonce"],
        "spec": {
            "region": "heteronet-global", "image": pod["spec"]["containers"][0]["image"],
            "replicas": 1, "cpu_millis": 100, "memory_mib": 128, "ephemeral_storage_gib": 3,
            "ports": [], "exposure": {"type": "internal", "traffic_mode": "forwarded"},
            "network": {"vpc_id": test["vpc_id"], "security_groups": ["workspaces"], "private_name": "iam-child"},
            "command": ["/bin/sleep"], "args": ["infinity"], "env": {}, "metadata": {},
        },
    }
    cli("flash", "create", "--no-wait", "--file", "-",
        body={**manifest, "spec": {**manifest["spec"], "task_role": test["principal_id"]}}, denied=True)
    record("parent_cannot_pass_a_role_without_iam_pass_role")
    children = [s for s in api.call("GET", "flash/services")["items"] if s["name"] == manifest["name"]]
    assert len(children) <= 1
    child = children[0] if children else cli("flash", "create", "--no-wait", "--file", "-", body=manifest)
    test["child_id"] = child["id"]
    args.fixture.write_text(json.dumps(fixture))
    ready = api.wait("flash/services/" + child["id"])
    assert ready["spec"].get("task_role") is None
    assert cli("flash", "get", child["id"])["state"] == "ready"
    child_pods = json.loads(kube("get", "pods", "-l", "flash.heterocloud.io/instance=" + child["id"], "-o", "json"))["items"]
    assert child_pods and all(p["spec"]["runtimeClassName"] == "gvisor" for p in child_pods)
    assert all(not p["metadata"].get("annotations", {}).get("iam.heterocloud.io/task-role") for p in child_pods)
    record("parent_creates_ready_private_child_in_vpc_without_inheriting_role", child_id=child["id"])

    # Retain one exchanged token in memory to check immediate revocation and
    # non-resurrection, in addition to fresh CLI exchanges inside the Pod.
    subject = kube("exec", name, "-c", "workload", "--", "sh", "-c", 'cat "$HETEROCLOUD_WORKLOAD_TOKEN_FILE"').decode().strip()
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    body = urllib.parse.urlencode({"grant_type": "urn:ietf:params:oauth:grant-type:token-exchange",
                                  "subject_token_type": "urn:ietf:params:oauth:token-type:jwt", "subject_token": subject}).encode()
    with opener.open(urllib.request.Request(test["endpoint"].rstrip("/") + "/api/v1/auth/workload/token", data=body,
                                           headers={"Content-Type": "application/x-www-form-urlencoded", "User-Agent": "HeteroCloud-VPC-E2E/1.0"}), timeout=20) as response:
        exchanged = json.load(response)
    assert exchanged["expires_in"] == 900
    token = exchanged["access_token"]
    del subject, body, exchanged

    def issued_token_status():
        request = urllib.request.Request(api.base + "/flash/services", headers={"Authorization": "Bearer " + token, "User-Agent": "HeteroCloud-VPC-E2E/1.0"})
        try:
            with opener.open(request, timeout=20) as response:
                return response.status
        except urllib.error.HTTPError as error:
            return error.code

    assert issued_token_status() == 200
    api.call("PATCH", "iam/principals/" + test["principal_id"], {"enabled": False})
    try:
        cli("iam", "whoami", denied=True)
        assert issued_token_status() in [401, 403]
        record("disabled_role_revokes_issued_tokens_and_rejects_new_exchanges")
    finally:
        api.call("PATCH", "iam/principals/" + test["principal_id"], {"enabled": True})
    assert issued_token_status() in [401, 403]
    assert cli("iam", "whoami")["type"] == "workload"
    record("role_reenable_requires_new_credentials_and_cannot_revive_old_tokens")
    api.call("DELETE", "iam/bindings/" + test["binding_id"])
    try:
        cli("flash", "get", child["id"], denied=True)
        record("binding_removal_applies_to_existing_workload_immediately")
    finally:
        binding = api.call("POST", "iam/bindings", {"principal_id": test["principal_id"], "policy_id": test["policy_id"]})
        test["binding_id"] = binding["id"]
        args.fixture.write_text(json.dumps(fixture))
    cli("flash", "delete", child["id"], "--yes", "--no-wait")
    api.wait("flash/services/" + child["id"], deleted=True)
    record("parent_deletes_child_using_task_role")
    report["passed"] = True
    report["observed_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    args.report.write_text(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
