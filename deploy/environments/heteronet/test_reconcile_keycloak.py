import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("reconcile-keycloak.sh")


@unittest.skipUnless(os.geteuid() == 0, "the reconciler enforces root ownership")
class PublicIdentityReconcileTests(unittest.TestCase):
    def test_preserves_public_cloud_login_without_recreating_network_owner_client(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            admin = root / "admin-password"
            secret = root / "client-secret"
            log = root / "calls.jsonl"
            fake = root / "kcadm"
            admin.write_text("test-only-admin-password\n")
            secret.write_text("test-only-client-secret-with-32-characters\n")
            fake.write_text('''#!/usr/bin/env python3
import json,os,sys
args=sys.argv[1:]
record={"args":args}
if args and args[0]=="update" and args[1].startswith("clients/"):
 data=json.load(open(args[args.index("-f")+1]))
 record["client"]={key:data.get(key) for key in ("clientId","enabled","redirectUris")}
with open(os.environ["TEST_KCADM_LOG"],"a") as stream: stream.write(json.dumps(record)+"\\n")
if args[:2]==["get","realms/heterocloud"]: print("{}")
elif args[:2]==["get","clients"]: print(json.dumps([{"id":"cloud-client-id"}]))
''')
            fake.chmod(0o700)
            env = os.environ.copy()
            env.update({"HETEROCLOUD_KEYCLOAK_ADMIN_PASSWORD_FILE": str(admin),
                        "HETEROCLOUD_OIDC_CLIENT_SECRET_FILE": str(secret),
                        "HETEROCLOUD_KCADM": str(fake), "TEST_KCADM_LOG": str(log)})
            subprocess.run(["bash", str(SCRIPT)], env=env, check=True, capture_output=True, text=True)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            client_updates = [call["client"] for call in calls if "client" in call]
            self.assertEqual(len(client_updates), 1)
            self.assertEqual(client_updates[0]["clientId"], "heterocloud-web")
            self.assertTrue(client_updates[0]["enabled"])
            self.assertIn("https://heterocloud.mizuame.app/api/v1/auth/oidc/callback",
                          client_updates[0]["redirectUris"])
            self.assertFalse(any("ipars-web" in " ".join(call["args"]) for call in calls))


if __name__ == "__main__":
    unittest.main()
