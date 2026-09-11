"""Rendered OIDC-only trust configuration; no cluster or credentials."""
import unittest

from test_owner_cookies import render, deployments


def settings(**extra):
    return {'enabled': True, 'issuerUrl': 'https://id.dev.example.test/realms/dev',
            'clientId': 'dev-web',
            'publicCallbackUrl': 'https://dev.example.test/api/v1/auth/oidc/callback', **extra}


class OidcCaTests(unittest.TestCase):
    def test_ca_reaches_api_and_owner_but_not_worker(self):
        baseline = render({}, oidc=settings())
        configured = render({}, oidc=settings(rootCaSecretName='dev-identity-ca', rootCaSecretKey='root.pem'))
        self.assertEqual(baseline.returncode, 0, baseline.stderr)
        self.assertEqual(configured.returncode, 0, configured.stderr)
        before, after = deployments(baseline.stdout), deployments(configured.stdout)
        workers = [name for name in after if name.endswith('-worker')]
        self.assertEqual(len(workers), 1)
        self.assertEqual(before[workers[0]], after[workers[0]])
        count = 0
        for name, deployment in after.items():
            if name in workers:
                continue
            pod = deployment['spec']['template']['spec']
            container = pod['containers'][0]
            self.assertIn('--oidc-root-ca-file=/var/run/secrets/oidc-ca/ca.crt', container['args'])
            mount = next(m for m in container['volumeMounts'] if m['name'] == 'oidc-ca')
            self.assertTrue(mount['readOnly'])
            volume = next(v for v in pod['volumes'] if v['name'] == 'oidc-ca')['secret']
            self.assertEqual(volume['secretName'], 'dev-identity-ca')
            self.assertEqual(volume['items'], [{'key': 'root.pem', 'path': 'ca.crt'}])
            self.assertFalse(volume.get('optional', False))
            self.assertFalse(any(e['name'] in ('SSL_CERT_FILE', 'SSL_CERT_DIR') for e in container.get('env', [])))
            count += 1
        self.assertEqual(count, 2)

    def test_invalid_or_unused_ca_configuration_is_rejected(self):
        for override in ({'rootCaSecretKey': ''}, {'rootCaSecretName': 123}, {'enabled': False}):
            result = render({}, oidc=settings(**{'rootCaSecretName': 'dev-identity-ca', **override}))
            self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
