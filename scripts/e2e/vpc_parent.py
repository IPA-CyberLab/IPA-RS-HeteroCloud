"""Disposable parent: creates one private child through HeteroCloud's API.

Only predefined fixture destinations can be probed. This is not a shell or a
general-purpose network proxy. Credentials never enter HTTP responses or logs.
"""
import http.server
import json
import os
import socket
import urllib.error
import urllib.request

opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def api(method, path, body=None):
    request = urllib.request.Request(os.environ["E2E_API_BASE"] + path, data=None if body is None else json.dumps(body).encode(), method=method, headers={"Authorization": "Bearer " + os.environ["HETEROCLOUD_API_KEY"], "Content-Type": "application/json", "User-Agent": "HeteroCloud-VPC-E2E/1.0"})
    with opener.open(request, timeout=12) as response:
        return json.load(response)


def run(probe):
    targets = json.loads(os.environ["E2E_TARGETS"])
    if probe in ("create", "forbidden-group"):
        manifest = json.loads(os.environ["E2E_CHILD_MANIFEST"])
        if probe == "forbidden-group":
            manifest["spec"]["network"]["security_groups"] = ["parents"]
            manifest["spec"]["network"]["private_name"] = "forbidden-child"
        else:
            for service in api("GET", "/flash/services")["items"]:
                if service["spec"].get("network", {}).get("private_name") == "child":
                    return {"ok": True, "child_id": service["id"], "state": service["state"]}
        child = api("POST", "/flash/services", manifest)
        return {"ok": True, "child_id": child["id"], "state": child["state"]}
    if probe == "nat":
        with opener.open(os.environ["E2E_TRACE_URL"], timeout=4) as response:
            text = response.read(8192).decode()
        address = next(line[3:] for line in text.splitlines() if line.startswith("ip="))
        return {"ok": True, "source_ip": address}
    if probe == "child-udp":
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
            connection.settimeout(3)
            connection.sendto(b"vpc-owned-fixture", (targets["child"], 8081))
            body, _ = connection.recvfrom(128)
            return {"ok": body == b"vpc-owned-fixture"}
    selected = {"child-tcp": ("child", 8080), "wrong-port": ("child", 8082), "other-group": ("bystander", 8080), "other-vpc": ("other-vpc", 8080), "other-org": ("other-org", 8080)}
    key, port = selected[probe]
    # Resolve before connecting so DNS failures cannot masquerade as policy denials.
    socket.getaddrinfo(targets[key], port, socket.AF_INET, socket.SOCK_STREAM)
    with opener.open(f"http://{targets[key]}:{port}/", timeout=3) as response:
        body = json.load(response)
    return {"ok": body["marker"] == os.environ["E2E_MARKER"], "parent_key_present": body["parent_key_present"]}


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self.send_response(200)
            self.end_headers()
            return
        prefix = "/" + os.environ["E2E_MARKER"] + "/"
        probe = self.path.removeprefix(prefix)
        if not self.path.startswith(prefix) or probe not in {"create", "forbidden-group", "nat", "child-tcp", "child-udp", "wrong-port", "other-group", "other-vpc", "other-org"}:
            self.send_response(404)
            self.end_headers()
            return
        try:
            result = run(probe)
        except urllib.error.HTTPError as error:
            result = {"ok": False, "http_status": error.code}
        except socket.gaierror:
            result = {"ok": False, "reason": "dns_failure"}
        except urllib.error.URLError as error:
            result = {"ok": False, "reason": "dns_failure" if isinstance(error.reason, socket.gaierror) else "connection_blocked"}
        except (TimeoutError, ConnectionError):
            result = {"ok": False, "reason": "connection_blocked"}
        except Exception as error:
            result = {"ok": False, "reason": type(error).__name__}
        body = json.dumps(result).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


http.server.ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
