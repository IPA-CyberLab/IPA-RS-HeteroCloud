"""Check public API termination leaves time for DNS withdrawal and draining."""
import subprocess
import unittest
from pathlib import Path

import yaml

CHART = Path(__file__).resolve().parents[1]


class ApiDrainTest(unittest.TestCase):
    def test_public_api_allows_dns_ttl_and_graceful_shutdown(self):
        result = subprocess.run(
            ['helm', 'template', 'drain-test', str(CHART)],
            text=True, capture_output=True, check=True,
        )
        deployment = next(
            resource for resource in yaml.safe_load_all(result.stdout)
            if resource and resource.get('kind') == 'Deployment'
            and resource['metadata']['name'] == 'drain-test-heterocloud'
        )
        pod = deployment['spec']['template']['spec']
        api = next(container for container in pod['containers'] if container['name'] == 'api')
        command = api['lifecycle']['preStop']['exec']['command']
        self.assertEqual(command[0], '/bin/sleep')
        withdrawal_seconds = int(command[1])
        # The cluster's headless Service currently advertises a 30-second DNS
        # TTL; graceful_shutdown itself can use a further 30 seconds.
        self.assertGreaterEqual(withdrawal_seconds, 30)
        self.assertGreaterEqual(pod['terminationGracePeriodSeconds'], withdrawal_seconds + 30)
        # A new healthy replica must remain available long enough for the
        # gateway to discover and health-check it before the previous replica
        # is taken out of service, even across two cached DNS answers.
        self.assertGreaterEqual(deployment['spec']['minReadySeconds'], 2 * 30 + 2)


if __name__ == '__main__':
    unittest.main()
