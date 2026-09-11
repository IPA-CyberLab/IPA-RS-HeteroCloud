"""Database CA references only. No cluster, credentials or TLS handshake."""
import json
from pathlib import Path
import subprocess
import unittest

import yaml

CHART = Path(__file__).resolve().parents[1]


def render(tls=None, **extra):
    command = ["helm", "template", "db-tls-test", str(CHART)]
    fixture = CHART / "ci/test-values.yaml"
    if fixture.exists():
        command += ["--values", str(fixture)]
    values = {"ownerConsole": {"enabled": True, "email": "owner@example.test"}}
    if tls is not None:
        values["databaseTls"] = tls
    values.update(extra)
    return subprocess.run(command + ["--values", "-"], input=json.dumps(values),
                          text=True, capture_output=True, timeout=60)


def containers(output):
    for doc in yaml.safe_load_all(output):
        if doc and doc["kind"] in ("Deployment", "StatefulSet", "Job"):
            for container in doc["spec"]["template"]["spec"]["containers"]:
                yield container


class DatabaseTlsTests(unittest.TestCase):
    def test_disabled_default(self):
        result = render()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(env["name"] == "PGSSLROOTCERT"
                             for c in containers(result.stdout) for env in c.get("env", [])))

    def test_all_database_clients_and_only_database_clients(self):
        result = render({"caSecretName": "dev-postgres-ca", "caSecretKey": "ca.crt"})
        self.assertEqual(result.returncode, 0, result.stderr)
        count = 0
        for c in containers(result.stdout):
            entries = [e for e in c.get("env", []) if e["name"] == "PGSSLROOTCERT"]
            database = any("database-url-file=" in arg for arg in c.get("args", [])) or any(
                e["name"] in ("DATABASE_URL", "SYOUYU_DATABASE_URL") for e in c.get("env", []))
            self.assertEqual(len(entries), int(database), c["name"])
            if entries:
                count += 1
                self.assertEqual(entries[0]["valueFrom"]["secretKeyRef"], {
                    "name": "dev-postgres-ca", "key": "ca.crt", "optional": False})
        self.assertEqual(count, 3)

    def test_invalid_reference_rejected(self):
        for tls in ({"caSecretName": 123}, {"caSecretName": "bad/name"},
                    {"caSecretName": "db-ca", "caSecretKey": ""},
                    {"caSecretName": "db-ca", "caSecretKey": "../ca.crt"}):
            with self.subTest(tls=tls):
                self.assertNotEqual(render(tls).returncode, 0)


if __name__ == "__main__":
    unittest.main()
