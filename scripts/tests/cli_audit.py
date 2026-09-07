#!/usr/bin/env python3
"""Bounded black-box audit of the published Linux x64 CLI; no production API calls.

Run: python3 scripts/tests/cli_audit.py --report /tmp/cli-audit.json
Only the pinned release download and reserved .invalid DNS lookups use networking
outside loopback. kubectl/helm are replaced with recording stubs, never real tools.
"""

import argparse
import hashlib
import http.server
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
URL = "https://github.com/IPA-CyberLab/IPA-RS-HeteroCloud/releases/download/v0.1.61/heterocloud-v0.1.61-linux-x64.tar.gz"
ARCHIVE_SHA = "41bc6c308581093e9ca1021c930332bec02391e54657ffaeb21bf1e4a0e0f491"
BINARY_SHA = "cf80f6b2c5a0669843eaba26ca24520bb2fbee7c135d8af45f362109119cc653"
ORG = "00000000-0000-4000-8000-000000000001"
PROJECT = "00000000-0000-4000-8000-000000000002"
ID = "00000000-0000-4000-8000-000000000003"
KEY = "hc_local_audit_dummy_not_a_secret"
KINDS = {"flow": "realtime/services", "flash": "flash/services", "syouyu": "syouyu/buckets"}


class Mock(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def handle_request(self):
        state = self.server.state
        body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
        request = {"method": self.command, "path": self.path,
                   "body": json.loads(body) if body else None,
                   "authorization": self.headers.get("Authorization"),
                   "user_agent": self.headers.get("User-Agent")}
        state["requests"].append(request)
        responses = state["responses"]
        index = min(len(state["requests"]) - 1, len(responses) - 1)
        status, payload, delay = responses[index]
        time.sleep(delay)
        if status == 0:
            self.close_connection = True
            return
        encoded = payload.encode() if isinstance(payload, str) else json.dumps(payload).encode()
        try:
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)
        except (BrokenPipeError, ConnectionResetError):
            pass

    do_GET = do_POST = do_PATCH = do_PUT = do_DELETE = handle_request


def service(kind, state="ready", **changes):
    return dict(id=ID, organization_id=ORG, project_id=PROJECT, provider=kind,
                name="audit", generation=2, state=state, spec={}, status={},
                created_at="2026-09-07T00:00:00Z", updated_at="2026-09-07T00:00:00Z",
                **changes)


def reply(value, status=200, delay=0):
    return status, value, delay


def error(status):
    return reply({"error": {"code": "audit_error", "message": "local fixture"}}, status)


class Audit:
    def __init__(self, binary, tmp, version="0.1.61"):
        self.binary, self.tmp, self.results = binary, tmp, []
        self.version = version
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Mock)
        self.server.daemon_threads = True
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.origin = f"http://127.0.0.1:{self.server.server_port}"
        self.env = {"PATH": str(tmp / "bin"), "HOME": str(tmp), "LANG": "C",
                    "NO_PROXY": "*", "KUBECONFIG": str(tmp / "nonexistent-kubeconfig"),
                    "AUDIT_LOG": str(tmp / "subprocess.jsonl")}
        self.defaults = ["--endpoint", self.origin, "--allow-insecure-http",
                         "--api-key", KEY, "--organization-id", ORG,
                         "--wait-timeout-seconds", "1"]

    def run(self, name, args, *, api=False, responses=None, rc=0, contains=None,
            methods=None, check=None, stdin="", env=None, timeout=12, defaults=None):
        self.server.state = {"requests": [], "responses": responses or [reply({})]}
        log = Path(self.env["AUDIT_LOG"])
        log.write_text("")
        full = [str(self.binary)] + (self.defaults if defaults is None and api else defaults or []) + args
        started = time.monotonic()
        proc = subprocess.Popen(full, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, env=self.env | (env or {}),
                                start_new_session=True, cwd=self.tmp)
        timed_out = False
        try:
            out, err = proc.communicate(stdin, timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(proc.pid, signal.SIGKILL)
            out, err = proc.communicate()
        elapsed = round(time.monotonic() - started, 3)
        requests = list(self.server.state["requests"])
        children = [json.loads(line) for line in log.read_text().splitlines()]
        failures = []
        if timed_out:
            failures.append(f"harness deadline {timeout}s exceeded")
        if proc.returncode not in (rc if isinstance(rc, tuple) else (rc,)):
            failures.append(f"expected exit {rc}, got {proc.returncode}")
        if contains is not None and contains not in out + err:
            failures.append(f"missing output: {contains}")
        if methods is not None and [r["method"] for r in requests] != methods:
            failures.append(f"expected request methods {methods}")
        for request in requests:
            if request["authorization"] != "Bearer " + KEY:
                failures.append("incorrect dummy bearer header")
            if request["user_agent"] != "heterocloud-cli/" + self.version:
                failures.append("incorrect user agent")
        if check:
            try:
                check(out, err, requests, children, elapsed)
            except (AssertionError, ValueError, KeyError, IndexError, TypeError) as exc:
                failures.append(str(exc) or "output/request assertion failed")
        result = dict(name=name, command=full, stdin=stdin, env=env or {},
                      expected_exit=rc, exit=proc.returncode, elapsed_seconds=elapsed,
                      response_fixtures=responses or [],
                      stdout=out, stderr=err, requests=requests, subprocesses=children,
                      failures=failures, passed=not failures)
        self.results.append(result)
        print(f"{'PASS' if not failures else 'FAIL'} {name}: {', '.join(failures)}", flush=True)
        return result


def require(condition, message):
    assert condition, message


def fixtures(tmp):
    (tmp / "bin").mkdir()
    stub = f"#!{sys.executable}\n" + '''import json, os, sys
from pathlib import Path
program = Path(sys.argv[0]).name
args = sys.argv[1:]
data = sys.stdin.read() if ("-" in args or "--filename=-" in args) else ""
with open(os.environ["AUDIT_LOG"], "a") as f:
    f.write(json.dumps({"program": program, "args": args, "stdin": data}) + "\\n")
if os.environ.get("AUDIT_FAIL") == program:
    print("local fixture command failure", file=sys.stderr)
    sys.exit(7)
if "get" in args:
    print(os.environ.get("AUDIT_DISCOVERY", '{"status":{"loadBalancer":{"ingress":[{"ip":"192.0.2.10"}]}}}'))
elif "create" in args:
    print('{"apiVersion":"v1","kind":"Secret","metadata":{"name":"audit"}}')
else:
    print("local fixture accepted")
'''
    for name in ("kubectl", "helm"):
        path = tmp / "bin" / name
        path.write_text(stub)
        path.chmod(0o700)
    for mode in (0o600, 0o400, 0o644):
        path = tmp / f"key-{mode:o}"
        path.write_text(KEY + "\n")
        path.chmod(mode)
    (tmp / "key-link").symlink_to(tmp / "key-600")
    (tmp / "dns-token").write_text(KEY)
    (tmp / "dns-token").chmod(0o600)
    (tmp / "provider-values.json").write_text('{"logLevel":"debug"}')


def wait_budget_cases(a):
    for command in ("create", "delete"):
        for scenario in ("persistent-pending", "unavailable-retry-backoff"):
            poll = reply(service("flow", "pending")) if scenario == "persistent-pending" else (
                503, {"error":{"code":"unavailable","message":"local fixture"}}, 0.4)
            a.run("wait:budget:" + command + ":" + scenario,
                  ["flow",command] + ([ID,"--yes"] if command == "delete" else []), api=True,
                  stdin=json.dumps({"project_id":PROJECT,"name":"audit","spec":{}}) if command == "create" else "",
                  responses=[reply(service("flow", "pending")), poll], rc=1, contains="did not converge",
                  check=lambda o,e,r,c,t: require(t < 1.75 and len(r) >= 2,
                                                 "1s wait exceeded 1.75s allowance or polling absent"))


def run_suite(a):
    # Discover the command tree from the binary, retaining every help transcript.
    pending = [[]]
    discovered = []
    while pending:
        path = pending.pop(0)
        result = a.run("help:" + (" ".join(path) or "root"), path + ["--help"], contains="Usage:")
        discovered.append(path)
        section = result["stdout"].split("Commands:\n")
        if len(section) > 1:
            for line in section[1].split("\n\n", 1)[0].splitlines():
                command = line.split()[0]
                if command != "help":
                    pending.append(path + [command])
    expected = {tuple([kind, cmd]) for kind in KINDS for cmd in ("list", "get", "create", "update", "delete")}
    expected |= {("dns", cmd) for cmd in ("records", "verify", "reconcile")}
    require(expected <= {tuple(p) for p in discovered}, "command inventory changed")
    a.run("version", ["--version"], contains="heterocloud " + a.version)
    a.run("short-version", ["-V"], contains="heterocloud " + a.version)
    a.run("short-help", ["-h"], contains="Usage:")
    a.run("help-command", ["help", "flow", "create"], contains="--no-wait")
    a.run("missing-command", [], rc=2, contains="Usage:")
    a.run("unknown-command", ["not-a-command"], rc=2, contains="unrecognized")

    for kind, suffix in KINDS.items():
        obj = service(kind)
        path = f"/api/v1/organizations/{ORG}/{suffix}"
        manifest = json.loads((ROOT / "examples" / "cli" / f"{kind}.json").read_text())
        manifest["project_id"] = PROJECT
        create = json.dumps(manifest)
        update = json.dumps({"name": manifest["name"], "spec": manifest["spec"]})
        (a.tmp / f"{kind}.json").write_text(create)
        (a.tmp / f"{kind}-update.json").write_text(update)

        def output_check(out, err, requests, children, elapsed, expected_path=path):
            require(requests[0]["path"] == expected_path, "wrong collection path")
            require(json.loads(out)[0]["id"] == ID, "wrong list output")

        a.run(f"{kind}:list", [kind, "list"], api=True, responses=[reply({"items": [obj]})],
              methods=["GET"], check=output_check)
        a.run(f"{kind}:list-project-table", [kind, "list", "--project-id", PROJECT, "--output", "table"],
              api=True, responses=[reply({"items": [obj]})], methods=["GET"], contains="ID\tNAME\tSTATE",
              check=lambda o,e,r,c,t,p=path: require(r[0]["path"] == p + "?project_id=" + PROJECT, "project filter not sent"))
        a.run(f"{kind}:list-empty", [kind, "list"], api=True, responses=[reply({"items": []})],
              methods=["GET"], check=lambda o,*_: require(json.loads(o) == [], "empty list shape"))
        for output in ("json", "table"):
            a.run(f"{kind}:get-{output}", [kind, "get", ID, "--output", output], api=True,
                  responses=[reply(obj)], methods=["GET"], contains=ID,
                  check=lambda o,e,r,c,t,p=path: require(r[0]["path"] == p + "/" + ID, "wrong item path"))
        for command, payload, method in (("create", create, "POST"),
                                         ("update", update, "PATCH" if kind == "flow" else "PUT")):
            base = [kind, command] + ([ID] if command == "update" else [])
            for source in ("stdin", "file"):
                file = "-" if source == "stdin" else str(a.tmp / f"{kind}{'-update' if command == 'update' else ''}.json")
                a.run(f"{kind}:{command}-{source}-no-wait", base + ["-f", file, "--no-wait"],
                      api=True, stdin=payload if source == "stdin" else "", responses=[reply(obj)], methods=[method],
                      check=lambda o,e,r,c,t,p=payload,route=path + ("/" + ID if command == "update" else ""):
                      require(r[0]["body"] == json.loads(p) and r[0]["path"] == route and json.loads(o)["id"] == ID,
                              "manifest, route, or returned ID differs"))
            a.run(f"{kind}:{command}-wait", base + ["--wait-timeout-seconds", "5"], api=True,
                  stdin=payload, responses=[reply(service(kind, "pending")), reply(service(kind, "pending")), reply(obj)],
                  methods=[method, "GET", "GET"], contains='"ready"')
            a.run(f"{kind}:{command}-error-state", base, api=True, stdin=payload,
                  responses=[reply(obj), reply(service(kind, "error"))], methods=[method, "GET"], rc=1, contains="error state")
            a.run(f"{kind}:{command}-timeout", base, api=True, stdin=payload,
                  responses=[reply(service(kind, "pending"))], rc=1, contains="did not converge",
                  check=lambda o,e,r,c,t,m=method: require([x["method"] for x in r] in ([m,"GET"],[m,"GET","GET"]), "unexpected polling requests"))
            a.run(f"{kind}:{command}-api-error-no-retry", base + ["--no-wait"], api=True,
                  stdin=payload, responses=[error(503)], rc=1, contains="HTTP 503", methods=[method])
        a.run(f"{kind}:delete-confirmation", [kind, "delete", ID], api=True, rc=1, contains="requires --yes", methods=[])
        a.run(f"{kind}:delete-no-wait", [kind, "delete", ID, "--yes", "--no-wait"], api=True,
              responses=[reply(service(kind, "deleting"))], methods=["DELETE"], contains='"deleting"')
        for output in ("json", "table"):
            a.run(f"{kind}:delete-wait-{output}", [kind, "delete", ID, "--yes", "--output", output], api=True,
                  responses=[reply(service(kind, "deleting")), error(404)], methods=["DELETE", "GET"], contains="true")
        a.run(f"{kind}:delete-timeout", [kind, "delete", ID, "--yes"], api=True,
              responses=[reply(service(kind, "deleting"))], rc=1, contains="did not converge",
              check=lambda o,e,r,c,t: require([x["method"] for x in r] in (["DELETE","GET"],["DELETE","GET","GET"]), "unexpected deletion polling"))
        a.run(f"{kind}:delete-error-state", [kind, "delete", ID, "--yes"], api=True,
              responses=[reply(obj), reply(service(kind, "error"))], methods=["DELETE", "GET"], rc=1, contains="error state")
        a.run(f"{kind}:delete-api-error", [kind, "delete", ID, "--yes"], api=True,
              responses=[error(503)], methods=["DELETE"], rc=1, contains="HTTP 503")
        for command in ("list", "get"):
            args = [kind, command] + ([ID] if command == "get" else [])
            good = reply(obj if command == "get" else {"items": [obj]})
            for status in (400, 401, 403, 404, 409, 422, 500):
                a.run(f"{kind}:{command}-http-{status}", args, api=True, responses=[error(status)],
                      rc=1, contains=f"HTTP {status}", methods=["GET"])
            for status in (429, 502, 503, 504):
                a.run(f"{kind}:{command}-retry-{status}", args, api=True,
                      responses=[error(status), error(status), good], methods=["GET"] * 3, contains=ID)
            a.run(f"{kind}:{command}-retry-exhausted", args, api=True, responses=[error(503)],
                  rc=1, contains="HTTP 503", methods=["GET"] * 3)
            a.run(f"{kind}:{command}-transport-retry", args, api=True,
                  responses=[(0, {}, 0), good], methods=["GET"] * 2, contains=ID)
            a.run(f"{kind}:{command}-bad-json", args, api=True, responses=[reply("not json")],
                  rc=1, contains="failed to process JSON", methods=["GET"])
        for payload in ("", "{", "[]", json.dumps({"name":"x", "spec": {}}),
                        json.dumps({"project_id":PROJECT,"name":" x","spec":{}}),
                        json.dumps({"project_id":PROJECT,"name":"x","spec":[]}),
                        create[:-1] + ', "unknown":true}'):
            a.run(f"{kind}:invalid-manifest-{len(a.results)}", [kind, "create", "--no-wait"],
                  api=True, stdin=payload, rc=1, contains="invalid service manifest", methods=[])
        a.run(f"{kind}:missing-file", [kind, "create", "-f", str(a.tmp / "missing")], api=True,
              rc=1, contains="failed to read input", methods=[])
        a.run(f"{kind}:invalid-id", [kind, "get", "bad-id"], api=True, rc=2, contains="invalid value", methods=[])

    # Shared global-option behavior, isolated from any inherited real credentials.
    base = ["--endpoint", a.origin, "--allow-insecure-http", "--organization-id", ORG]
    good = [reply({"items": [service("flow")]})]
    a.run("auth:missing", ["flow", "list"], defaults=base, rc=1, contains="require HETEROCLOUD_API_KEY", methods=[])
    a.run("organization:missing", ["flow", "list"], defaults=["--endpoint", a.origin, "--api-key", KEY],
          rc=1, contains="require HETEROCLOUD_ORGANIZATION_ID", methods=[])
    for mode in ("600", "400", "644", "link", "missing"):
        valid = mode in ("600", "400")
        a.run(f"auth:file-{mode}", ["flow", "list", "--api-key-file", str(a.tmp / ("key-" + mode))],
              defaults=base, responses=good, rc=0 if valid else 1, methods=["GET"] if valid else [])
    a.run("auth:conflict", ["flow", "list", "--api-key-file", str(a.tmp / "key-600")], api=True,
          rc=(1, 2), methods=[])
    a.run("auth:conflict-same-level", ["flow", "list", "--api-key", KEY,
          "--api-key-file", str(a.tmp / "key-600")], defaults=base,
          rc=2, contains="cannot be used with", methods=[])
    a.run("auth:invalid", ["flow", "list", "--api-key", "dummy-invalid"], defaults=base,
          rc=1, contains="API key must start with hc_", methods=[])
    environment = {"HETEROCLOUD_ENDPOINT":a.origin, "HETEROCLOUD_API_KEY":KEY,
                   "HETEROCLOUD_ORGANIZATION_ID":ORG, "HETEROCLOUD_ALLOW_INSECURE_HTTP":"true",
                   "HETEROCLOUD_WAIT_TIMEOUT_SECONDS":"1"}
    a.run("globals:environment", ["flow", "list"], env=environment, responses=good, methods=["GET"])
    a.run("globals:environment-key-file", ["flow", "list"],
          env={k:v for k,v in environment.items() if k != "HETEROCLOUD_API_KEY"} |
              {"HETEROCLOUD_API_KEY_FILE": str(a.tmp / "key-600")}, responses=good, methods=["GET"])
    a.run("globals:flags-override-env", ["flow", "list"], api=True,
          env=environment | {"HETEROCLOUD_ENDPOINT":"http://127.0.0.1:1"}, responses=good, methods=["GET"])
    a.run("endpoint:http-opt-in", ["flow", "list"],
          defaults=["--endpoint", a.origin, "--api-key", KEY, "--organization-id", ORG],
          rc=1, contains="plain HTTP requires", methods=[])
    for endpoint in ("not-url", "ftp://127.0.0.1", a.origin + "?query=x", a.origin + "#fragment"):
        a.run("endpoint:invalid:" + endpoint, ["flow", "list"],
              defaults=["--endpoint", endpoint, "--api-key", KEY, "--organization-id", ORG],
              rc=1, contains="invalid API endpoint", methods=[])
    for option, value in (("--wait-timeout-seconds", "0"), ("--wait-timeout-seconds", "7201"),
                          ("--output", "yaml"), ("--organization-id", "bad")):
        a.run("global:invalid:" + option + value, ["flow", "list", option, value], rc=2, methods=[])
    a.run("api:unstructured-error", ["flow", "get", ID], api=True, responses=[reply("fixture unavailable", 502)],
          rc=1, contains="unexpected_response", methods=["GET"] * 3)
    a.run("api:provider-mismatch", ["flow", "get", ID], api=True, responses=[reply(service("flash"))],
          rc=1, contains="provider_mismatch", methods=["GET"])
    a.run("wait:deadline-late-ready", ["flow", "create"], api=True,
          stdin=json.dumps({"project_id":PROJECT,"name":"audit","spec":{}}),
          responses=[reply(service("flow", "pending")), reply(service("flow"), delay=2)],
          rc=1, contains="did not converge", methods=["POST", "GET"],
          check=lambda o,e,r,c,t: require(t < 1.75, "1s wait exceeded 1.75s scheduling allowance"))
    a.run("wait:deadline-late-deleted", ["flow", "delete", ID, "--yes"], api=True,
          responses=[reply(service("flow", "deleting")), reply({}, status=404, delay=2)],
          rc=1, contains="did not converge", methods=["DELETE", "GET"],
          check=lambda o,e,r,c,t: require(t < 1.75, "1s wait exceeded 1.75s scheduling allowance"))
    for command in ("create", "delete"):
        a.run("wait:deadline-late-error:" + command,
              ["flow",command] + ([ID,"--yes"] if command == "delete" else []), api=True,
              stdin=json.dumps({"project_id":PROJECT,"name":"audit","spec":{}}) if command == "create" else "",
              responses=[reply(service("flow", "pending")), reply(service("flow", "error"), delay=2)],
              rc=1, contains="did not converge", methods=["POST" if command == "create" else "DELETE", "GET"],
              check=lambda o,e,r,c,t: require(t < 1.75, "1s wait exceeded 1.75s scheduling allowance"))
    a.run("request:30-second-timeout-no-write-retry", ["flow", "create", "--no-wait"], api=True,
          stdin=json.dumps({"project_id":PROJECT,"name":"audit","spec":{}}),
          responses=[reply(service("flow"), delay=31)], rc=1, contains="API request failed", methods=["POST"], timeout=35)

    dns = ["--domain", "audit.invalid", "--public-ip", "192.0.2.10", "--allow-non-public"]
    for fmt in ("zone", "table", "json"):
        a.run("dns:records-" + fmt, ["dns", "records"] + dns + ["--format", fmt, "--ttl", "120"],
              contains="cloud-a.audit.invalid", methods=[])
    a.run("dns:records-documented-s3", ["dns", "records"] + dns + ["--format", "json"],
          check=lambda o,*_: require("s3.audit.invalid" in {r["name"] for r in json.loads(o)},
                                      "README promises s3.<domain>; generated records omit it"))
    a.run("dns:records-dedup-sort", ["dns", "records"] + dns +
          ["--public-ip", "192.0.2.11", "--public-ip", "192.0.2.10", "--format", "json"],
          check=lambda o,*_: require(len(json.loads(o)) == len({(r["name"],r["value"]) for r in json.loads(o)})
                                     and json.loads(o)[0]["value"] == "192.0.2.10"
                                     and {r["value"] for r in json.loads(o)} == {"192.0.2.10","192.0.2.11"}, "dedup/sort differs"))
    discovery = ["dns", "records", "--domain", "audit.invalid", "--allow-non-public", "--kubeconfig", str(a.tmp / "fake"),
                 "--context", "audit", "--namespace", "audit-flow", "--service", "audit-turn"]
    a.run("dns:discovery-stub", discovery, contains="192.0.2.10",
          check=lambda o,e,r,c,t: require(c[0]["args"] == ["--kubeconfig",str(a.tmp / "fake"),"--context","audit",
                 "get","service","audit-turn","--namespace","audit-flow","--output=json"], "discovery args differ"))
    a.run("dns:discovery-error", discovery, env={"AUDIT_FAIL":"kubectl"}, rc=1, contains="kubectl failed")
    for payload in ("{}", "bad json", '{"status":{"loadBalancer":{"ingress":[{"ip":"bad"}]}}}'):
        a.run("dns:discovery-invalid:" + payload, discovery, env={"AUDIT_DISCOVERY":payload}, rc=1)
    a.run("dns:verify-reserved-negative", ["dns", "verify"] + dns, rc=1, contains="DNS verification failed", timeout=20)
    for args in (["--domain","localhost","--public-ip","192.0.2.10"],
                 ["--domain","audit.invalid","--public-ip","192.0.2.10"]):
        a.run("dns:invalid-source:" + str(args), ["dns","records"] + args, rc=1)
    for option, value in (("--ttl","29"),("--ttl","86401"),("--format","yaml"),("--public-ip","::1")):
        a.run("dns:invalid-option:" + option + value, ["dns","records"] + dns + [option,value], rc=2)
    reconcile = ["dns","reconcile"] + dns + ["--provider","cloudflare"]
    for provider in ("cloudflare", "aws", "google", "rfc2136", "webhook"):
        a.run("dns:dry-run:" + provider, ["dns","reconcile"] + dns + ["--provider",provider,"--dry-run"],
              contains="DNSEndpoint", check=lambda o,e,r,c,t: require(not c, "dry run spawned cluster commands"))
    a.run("dns:reconcile-options-dry-run", reconcile + ["--dry-run", "--managed-zone","invalid",
          "--credential-secret","CF_API_TOKEN=audit:token"], rc=1, contains="invalid DNS domain")
    a.run("dns:reconcile-all-options", reconcile + ["--dry-run", "--managed-zone","audit.invalid",
          "--credential-file", "CF_API_TOKEN=" + str(a.tmp / "dns-token"),
          "--credential-secret","OTHER_TOKEN=audit:token", "--provider-arg=--cloudflare-dns-records-per-page=100",
          "--http-edge-property","external-dns.alpha.kubernetes.io/cloudflare-proxied=true",
          "--provider-values",str(a.tmp / "provider-values.json"),
          "--controller-kube-api-server","https://api.audit.invalid:6443", "--controller-node-selector","role=worker",
          "--controller-dns-policy","Default", "--controller-failover-seconds","20", "--controller-namespace","audit-dns",
          "--controller-release","audit-dns", "--credential-secret-name","audit-provider", "--endpoint-name","audit-public",
          "--http-route-namespace","audit", "--http-route-name","audit-public", "--chart-version","1.21.1",
          "--txt-owner-id","audit-owner", "--ttl","120", "--timeout-seconds","30", "--no-wait-dns"],
          contains="audit-owner", check=lambda o,e,r,c,t: require(KEY not in o and not c, "dry run not sanitized/local"))
    a.run("dns:reconcile-stub-apply", reconcile + ["--no-wait-dns"], contains="DNSEndpoint applied",
          check=lambda o,e,r,c,t: require(len(c) == 7 and sum(x["program"] == "helm" for x in c) == 2,
                                         "expected namespace, route, helm repo/install, endpoint, restart/status"))
    a.run("dns:reconcile-local-credential-stub", reconcile + ["--no-wait-dns", "--credential-file",
          "CF_API_TOKEN=" + str(a.tmp / "dns-token")], contains="DNSEndpoint applied")
    for program in ("kubectl", "helm"):
        a.run("dns:reconcile-child-error:" + program, reconcile + ["--no-wait-dns"],
              env={"AUDIT_FAIL":program}, rc=1, contains="local fixture command failure")
    a.run("dns:reconcile-convergence-timeout", reconcile + ["--timeout-seconds","30"],
          rc=1, contains="DNS did not converge within 30 seconds", timeout=45)
    for option, value in (("--provider","bad provider"),("--managed-zone","other.invalid"),
                          ("--credential-file","bad"),("--credential-secret","bad"),
                          ("--controller-dns-policy","bad"),("--controller-node-selector","bad"),
                          ("--chart-version","bad version"),("--provider-values",str(a.tmp / "missing"))):
        args = (["dns","reconcile"] + dns if option == "--provider" else reconcile) + ["--dry-run",option,value]
        a.run("dns:reconcile-invalid:" + option, args, rc=1)
    wait_budget_cases(a)
    return discovered


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="Local or future Linux build; actual version and digest recorded")
    parser.add_argument("--wait-budget-only", action="store_true", help="Run only four pending/retry budget regressions")
    parser.add_argument("--report", type=Path, default=Path("/tmp/heterocloud-cli-audit-0161.json"))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="heterocloud-cli-audit-") as directory:
        tmp = Path(directory)
        binary = args.binary.resolve() if args.binary else tmp / "heterocloud"
        if not args.binary:
            with urllib.request.urlopen(URL, timeout=60) as response:
                archive = response.read(8 * 1024 * 1024)
            require(hashlib.sha256(archive).hexdigest() == ARCHIVE_SHA, "release archive checksum differs")
            with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
                binary.write_bytes(tar.extractfile("heterocloud").read())
            binary.chmod(0o700)
        actual_sha = hashlib.sha256(binary.read_bytes()).hexdigest()
        if not args.binary:
            require(actual_sha == BINARY_SHA, "release binary checksum differs")
        version_result = subprocess.run([str(binary), "--version"], capture_output=True,
                                        text=True, timeout=10, check=True,
                                        env={"HOME":str(tmp),"PATH":"","LANG":"C"})
        version = version_result.stdout.strip().removeprefix("heterocloud ")
        fixtures(tmp)
        audit = Audit(binary, tmp, version)
        discovered = []
        try:
            if args.wait_budget_only:
                wait_budget_cases(audit)
            else:
                discovered = run_suite(audit)
        finally:
            audit.server.shutdown()
            audit.server.server_close()
            report = {"release_url":URL if not args.binary else None,
                      "archive_sha256":ARCHIVE_SHA if not args.binary else None,
                      "binary_sha256":actual_sha,"version":version_result.stdout.strip(),
                      "binary_source":"supplied" if args.binary else "pinned download",
                      "scope":"wait-budget-only" if args.wait_budget_only else "full",
                      "command_tree":discovered,"total":len(audit.results),
                      "passed":sum(r["passed"] for r in audit.results),
                      "failed":sum(not r["passed"] for r in audit.results),
                      "live_api_commands":0,"results":audit.results}
            args.report.parent.mkdir(parents=True, exist_ok=True)
            args.report.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({k:v for k,v in report.items() if k not in ("results","command_tree")}))
        return 1 if report["failed"] else 0


if __name__ == "__main__":
    sys.exit(main())
