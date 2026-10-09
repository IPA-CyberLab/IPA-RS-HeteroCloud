import copy
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "coder_scheduling", Path(__file__).resolve().parents[1] / "reconcile-coder-scheduling.py"
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
POLICY = {"default_ttl_ms": 0, "allow_user_autostop": True}


class Client:
    def __init__(self):
        self.template = {"default_ttl_ms": 28_800_000, "active_version_id": "active"}
        self.workspaces = [
            self.workspace("auto", "autostop", "stopped"),
            self.workspace("manual", "dashboard", "stopped"),
            self.workspace("running", "dashboard", "running"),
            self.workspace("foreign", "autostop", "stopped", owner="other"),
            self.workspace("other-template", "autostop", "stopped", template="other"),
        ]
        self.mutations = []

    @staticmethod
    def workspace(name, reason, status, owner="me", template="template"):
        return {"id": name, "name": name, "owner_id": owner, "template_id": template,
                "ttl_ms": 28_800_000, "latest_build": {"reason": reason, "status": status}}

    def call(self, path, method="GET", body=None):
        if method != "GET":
            self.mutations.append((method, path, body))
        if path == "/api/v2/users/me":
            return {"id": "me", "username": "user"}
        if path == "/api/v2/templates/template":
            if method == "PATCH":
                self.template.update(body)
            return copy.deepcopy(self.template)
        if path.startswith("/api/v2/workspaces?"):
            return {"workspaces": copy.deepcopy(self.workspaces)}
        identifier = path.split("/")[4]
        workspace = next(w for w in self.workspaces if w["id"] == identifier)
        if path.endswith("/ttl"):
            workspace["ttl_ms"] = body["ttl_ms"]
            return None
        if path.endswith("/builds"):
            workspace["latest_build"] = {"status": "starting", "reason": "dashboard"}
            return {"id": "started"}
        return copy.deepcopy(workspace)


class SchedulingTests(unittest.TestCase):
    def test_only_autostopped_owned_workspaces_resume(self):
        client = Client()
        report = module.reconcile(client, "template", POLICY, apply=True, resume=True)
        starts = [p for m, p, b in client.mutations if p.endswith("/builds")]
        self.assertEqual(starts, ["/api/v2/workspaces/auto/builds"])
        self.assertEqual(len(report["workspaces"]), 3)
        self.assertEqual(client.template["default_ttl_ms"], 0)
        self.assertEqual(client.workspaces[1]["latest_build"]["status"], "stopped")
        self.assertEqual(client.workspaces[2]["latest_build"]["status"], "running")
        self.assertEqual(client.workspaces[3]["ttl_ms"], 28_800_000)

    def test_repeated_apply_does_not_start_another_build(self):
        client = Client()
        module.reconcile(client, "template", POLICY, apply=True, resume=True)
        client.mutations.clear()
        module.reconcile(client, "template", POLICY, apply=True, resume=True)
        self.assertFalse(any(p.endswith("/builds") for m, p, b in client.mutations))
        self.assertFalse(any(p.endswith("/ttl") for m, p, b in client.mutations))

    def test_dry_run_does_not_mutate(self):
        client = Client()
        module.reconcile(client, "template", POLICY, apply=False, resume=True)
        self.assertEqual(client.mutations, [])

    def test_resume_must_be_requested(self):
        client = Client()
        module.reconcile(client, "template", POLICY, apply=True, resume=False)
        self.assertFalse(any(p.endswith("/builds") for m, p, b in client.mutations))


if __name__ == "__main__":
    unittest.main()
