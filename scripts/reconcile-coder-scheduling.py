#!/usr/bin/env python3
"""Apply declarative Coder scheduling without restarting running workspaces."""
import argparse
import json
import stat
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Client:
    def __init__(self, endpoint, token_file):
        parsed = urllib.parse.urlsplit(endpoint)
        if (parsed.scheme not in ("http", "https") or not parsed.netloc
                or parsed.username or parsed.password or parsed.query or parsed.fragment):
            raise ValueError("Use an explicit Coder endpoint without credentials or query parameters")
        path = Path(token_file)
        if path.is_symlink() or stat.S_IMODE(path.stat().st_mode) & 0o077:
            raise ValueError("The token file must be private to its owner")
        self.token = path.read_text().strip()
        if not self.token or "\n" in self.token:
            raise ValueError("The token file must contain one session token")
        self.endpoint = endpoint.rstrip("/")
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

    def call(self, path, method="GET", body=None):
        request = urllib.request.Request(
            self.endpoint + path, method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers={"Coder-Session-Token": self.token, "Content-Type": "application/json"},
        )
        try:
            with self.opener.open(request, timeout=60) as response:
                data = response.read()
        except urllib.error.HTTPError as error:
            raise RuntimeError(f"Coder API {method} {path}: HTTP {error.code}") from None
        return json.loads(data) if data else None


def reconcile(client, template_id, policy, apply=False, resume=False):
    if policy != {"default_ttl_ms": 0, "allow_user_autostop": True}:
        raise ValueError("This reconciler requires disabled default autostop with user opt-in allowed")
    user = client.call("/api/v2/users/me")
    template_path = "/api/v2/templates/" + template_id
    template = client.call(template_path)
    query = urllib.parse.urlencode({"q": "owner:" + user["username"], "limit": 1000})
    workspaces = client.call("/api/v2/workspaces?" + query)["workspaces"]
    workspaces = [w for w in workspaces
                  if w["owner_id"] == user["id"] and w["template_id"] == template_id]
    if apply:
        client.call(template_path, "PATCH", policy)
        template = client.call(template_path)
        if template["default_ttl_ms"] != 0:
            raise RuntimeError("The template's default autostop was not disabled")
    rows = []
    for workspace in workspaces:
        path = "/api/v2/workspaces/" + workspace["id"]
        current = client.call(path)
        if current["owner_id"] != user["id"] or current["template_id"] != template_id:
            raise RuntimeError("Workspace ownership or template changed during reconciliation")
        row = {"workspace": current["name"], "id": current["id"],
               "previous_ttl_ms": current.get("ttl_ms"), "autostop_disabled": apply}
        if apply and current.get("ttl_ms") not in (None, 0):
            client.call(path + "/ttl", "PUT", {"ttl_ms": 0})
            current = client.call(path)
            if current.get("ttl_ms") not in (None, 0):
                raise RuntimeError("The workspace's autostop was not disabled")
        build = current["latest_build"]
        should_resume = (resume and build["status"] == "stopped"
                         and build.get("reason") == "autostop")
        row["resume_autostopped"] = should_resume
        if apply and should_resume:
            started = client.call(path + "/builds", "POST", {
                "transition": "start", "template_version_id": template["active_version_id"],
                "reason": "dashboard",
            })
            row["started_build_id"] = started["id"]
        rows.append(row)
    return {"template_id": template_id, "applied": apply, "workspaces": rows}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--template-id", required=True, type=uuid.UUID)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--policy", type=Path,
                        default=Path(__file__).resolve().parents[1] / "examples/coder/scheduling.json")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--resume-autostopped", action="store_true")
    args = parser.parse_args()
    result = reconcile(Client(args.endpoint, args.token_file), str(args.template_id),
                       json.loads(args.policy.read_text()), args.apply, args.resume_autostopped)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
