"""Render SSH settings without accessing a cluster."""
import json
from pathlib import Path
import subprocess
import unittest
import yaml

CHART = Path(__file__).resolve().parents[1]


def render(values):
    result = subprocess.run(["helm", "template", "test", str(CHART), "--values", "-"],
                            input=json.dumps(values), text=True, capture_output=True, check=True)
    return [item for item in yaml.safe_load_all(result.stdout) if item]


class FlashSshChart(unittest.TestCase):
    def test_disabled_by_default(self):
        docs = render({})
        api = next(x for x in docs if x["kind"] == "Deployment" and x["metadata"]["name"] == "test-heterocloud")
        pod = api["spec"]["template"]["spec"]
        self.assertFalse(any(volume["name"] == "flash-ssh-host-key" for volume in pod["volumes"]))
        self.assertFalse(any(x["kind"] == "Service" and x["metadata"]["name"].endswith("-flash-ssh") for x in docs))

    def test_host_key_and_vpn_tcp_entry(self):
        docs = render({"flashSsh":{"enabled":True,"hostKeySecretName":"test-host-key","tcp":{
            "enabled":True,"port":2222,"overlayDnsName":"ssh.example.heteronetwork.internal",
            "advertisedHost":"ssh.example.heteronetwork.internal"}}})
        api = next(x for x in docs if x["kind"] == "Deployment" and x["metadata"]["name"] == "test-heterocloud")
        pod = api["spec"]["template"]["spec"]
        volume = next(v for v in pod["volumes"] if v["name"] == "flash-ssh-host-key")
        self.assertEqual(volume["secret"]["secretName"], "test-host-key")
        self.assertEqual(volume["secret"]["defaultMode"], 0o400)
        args = pod["containers"][0]["args"]
        self.assertIn("--flash-ssh-host-key-file=/var/run/secrets/flash-ssh/host-key", args)
        self.assertIn("--flash-ssh-listen=$(HOST_IP):2222", args)
        svc = next(x for x in docs if x["kind"] == "Service" and x["metadata"]["name"].endswith("-flash-ssh"))
        self.assertEqual(svc["metadata"]["annotations"]["networking.heteronetwork.io/overlay-dns-name"], "ssh.example.heteronetwork.internal")
        self.assertEqual(svc["spec"]["type"], "ClusterIP")
        self.assertNotIn("externalIPs", svc["spec"])


if __name__ == "__main__":
    unittest.main()
