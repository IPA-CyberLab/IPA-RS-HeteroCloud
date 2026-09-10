"""Owner cookie policy checks using local Helm only; no credentials or cluster."""

import json
from pathlib import Path
import subprocess
import unittest

import yaml


CHART = Path(__file__).resolve().parents[1]


def render(owner, **overrides):
    return subprocess.run(
        ["helm", "template", "owner-cookies-test", str(CHART), "--values", "-"],
        input=json.dumps({"ownerConsole": {"enabled": True, "email": "owner@example.test", **owner},
                          **overrides}),
        capture_output=True, text=True, check=False, timeout=30,
    )


def deployments(output):
    return {item["metadata"]["name"]: item for item in yaml.safe_load_all(output)
            if item and item["kind"] == "Deployment"}


class OwnerCookieTests(unittest.TestCase):
    def test_http_vpn_default_preserves_explicit_false(self):
        default = render({})
        explicit = render({"secureCookie": False})
        self.assertEqual(default.returncode, 0, default.stderr)
        self.assertEqual(explicit.returncode, 0, explicit.stderr)
        self.assertEqual(default.stdout, explicit.stdout)
        owner = deployments(default.stdout)["owner-cookies-test-heterocloud-owner-console"]
        self.assertIn("--secure-cookie=false", owner["spec"]["template"]["spec"]["containers"][0]["args"])

    def test_https_dev_sets_secure_cookie_without_changing_other_workloads(self):
        baseline = render({})
        secure = render({"origin": "https://owner.dev.example.test", "secureCookie": True,
                         "oidcPublicCallbackUrl": "https://owner.dev.example.test/api/v1/auth/oidc/callback"})
        self.assertEqual(secure.returncode, 0, secure.stderr)
        before, after = deployments(baseline.stdout), deployments(secure.stdout)
        owner_name = "owner-cookies-test-heterocloud-owner-console"
        args = after.pop(owner_name)["spec"]["template"]["spec"]["containers"][0]["args"]
        self.assertIn("--secure-cookie=true", args)
        self.assertNotIn("--secure-cookie=false", args)
        before.pop(owner_name)
        self.assertEqual(after, before)

    def test_https_never_inherits_insecure_owner_default(self):
        for scheme in ("https", "HTTPS"):
            for settings in ({}, {"secureCookie": False}):
                with self.subTest(scheme=scheme, settings=settings):
                    result = render({"origin": scheme + "://owner.dev.example.test", **settings})
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("ownerConsole.secureCookie", result.stderr)

    def test_https_dev_oidc_keeps_public_callback_and_secure_cookie(self):
        callback = "https://owner.dev.example.test/api/v1/auth/oidc/callback"
        result = render({"origin": "https://owner.dev.example.test", "secureCookie": True,
                         "oidcPublicCallbackUrl": callback},
                        oidc={"enabled": True, "issuerUrl": "https://id.dev.example.test/realms/heterocloud-dev",
                              "clientId": "heterocloud-dev-web",
                              "publicCallbackUrl": "https://dev.example.test/api/v1/auth/oidc/callback"})
        self.assertEqual(result.returncode, 0, result.stderr)
        owner = deployments(result.stdout)["owner-cookies-test-heterocloud-owner-console"]
        args = owner["spec"]["template"]["spec"]["containers"][0]["args"]
        self.assertIn("--secure-cookie=true", args)
        self.assertIn("--oidc-public-callback-url=" + callback, args)

    def test_cookie_policy_rejects_invalid_types_and_http_secure(self):
        for settings in ({"secureCookie": True}, {"secureCookie": "false"},
                         {"secureCookie": 0}, {"secureCookie": None},
                         {"origin": "ftp://owner.example.test"}):
            with self.subTest(settings=settings):
                result = render(settings)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("ownerConsole", result.stderr)

    def test_disabled_owner_does_not_change_main_cookie_policy(self):
        result = render({"enabled": False, "origin": "https://owner.dev.example.test"}, secureCookie=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        pods = deployments(result.stdout)
        self.assertNotIn("owner-cookies-test-heterocloud-owner-console", pods)
        api = pods["owner-cookies-test-heterocloud"]
        self.assertIn("--secure-cookie=true", api["spec"]["template"]["spec"]["containers"][0]["args"])


if __name__ == "__main__":
    unittest.main()
