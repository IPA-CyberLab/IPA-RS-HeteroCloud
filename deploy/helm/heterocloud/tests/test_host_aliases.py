"""Local Helm rendering only; no Kubernetes context or deployment is used."""

import json
from pathlib import Path
import subprocess
import unittest

import yaml


CHART = Path(__file__).resolve().parents[1]
ALIASES = [
    {"ip": "192.0.2.10", "hostnames": ["oidc.example.test", "login.example.test"]},
    {"ip": "2001:db8::10", "hostnames": ["oidc-v6.example.test"]},
]


def render(overrides):
    return subprocess.run(
        ["helm", "template", "host-aliases-test", str(CHART), "--values", "-"],
        input=json.dumps(overrides), capture_output=True, text=True, check=False,
    )


def deployments(output):
    return {
        item["metadata"]["name"]: item["spec"]["template"]["spec"]
        for item in yaml.safe_load_all(output)
        if item and item["kind"] == "Deployment"
    }


class HostAliasesTest(unittest.TestCase):
    def test_defaults_and_explicit_empty_leave_pods_unchanged(self):
        for owner_enabled in (False, True):
            with self.subTest(owner_enabled=owner_enabled):
                options = {"ownerConsole": {"enabled": owner_enabled, "email": "owner@example.test"}}
                default = render(options)
                empty = render({**options, "hostAliases": []})
                self.assertEqual(default.returncode, 0, default.stderr)
                self.assertEqual(empty.returncode, 0, empty.stderr)
                self.assertEqual(default.stdout, empty.stdout)
                pods = deployments(default.stdout)
                self.assertEqual(len(pods), 3 if owner_enabled else 2)
                for pod in pods.values():
                    self.assertNotIn("hostAliases", pod)

    def test_aliases_only_change_api_and_owner_pods(self):
        for host_network in (False, True):
            with self.subTest(host_network=host_network):
                options = {
                    "hostNetwork": host_network,
                    "ownerConsole": {"enabled": True, "email": "owner@example.test", "hostNetwork": host_network},
                }
                baseline = render(options)
                configured = render({**options, "hostAliases": ALIASES})
                self.assertEqual(baseline.returncode, 0, baseline.stderr)
                self.assertEqual(configured.returncode, 0, configured.stderr)
                before = deployments(baseline.stdout)
                after = deployments(configured.stdout)
                api = "host-aliases-test-heterocloud"
                self.assertEqual(set(after), {api, api + "-owner-console", api + "-worker"})
                for name in (api, api + "-owner-console"):
                    self.assertEqual(after[name].pop("hostAliases"), ALIASES)
                self.assertEqual(after, before)

    def test_invalid_values_fail_helm_schema_validation(self):
        invalid = [
            "not-a-list", {}, ["not-an-object"], [{}],
            [{"ip": "192.0.2.10"}], [{"hostnames": ["oidc.example.test"]}],
            [{"ip": "192.0.2.10", "hostnames": [], "extra": True}],
        ]
        for ip in ("", "999.0.0.1", "192.0.2.10/32", "oidc.example.test", "2001:db8::xyz", 123):
            invalid.append([{"ip": ip, "hostnames": ["oidc.example.test"]}])
        for hostnames in ([], "oidc.example.test", [123], [""], ["https://oidc.example.test"],
                          ["bad name"], ["bad_name"], ["-bad"], ["bad-"], ["bad..name"],
                          ["*.example.test"], ["a" * 64], ["a." * 127 + "a"]):
            invalid.append([{"ip": "192.0.2.10", "hostnames": hostnames}])
        for aliases in invalid:
            with self.subTest(aliases=aliases):
                result = render({"hostAliases": aliases})
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("hostAliases", result.stderr)


if __name__ == "__main__":
    unittest.main()
