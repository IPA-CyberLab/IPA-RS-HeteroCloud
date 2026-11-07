import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'examples/coder/wait-for-flash.sh'
ACCOUNT = '019fbda8-eff3-7610-bdd5-8fe89e746c14'
SERVICE = '01a1a54f-f1ea-7a73-88ac-5ab146ee960d'
SECRET = 'do-not-log-this-workspace-secret'


def service(phase, *, observed=4, generation=4, ready=0, code='', message=''):
    return {'id': SERVICE, 'generation': generation, 'spec': {'env': {'TOKEN': SECRET}},
            'status': {'status': {'phase': phase, 'observed_generation': observed,
                                  'ready_replicas': ready, 'message': message,
                                  'diagnostics': [{'code': code}] if code else []}}}


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.seen += 1
        if self.path != f'/api/v1/organizations/{ACCOUNT}/flash/services/{SERVICE}':
            self.send_error(404)
            return
        response = self.server.responses[min(self.server.seen - 1, len(self.server.responses) - 1)]
        if response is None:
            self.send_error(503)
            return
        body = json.dumps(response).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


class WaitForFlashTest(unittest.TestCase):
    def run_wait(self, responses, seconds=8):
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.responses = responses
        server.seen = 0
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        env = os.environ.copy()
        env.update(HETEROCLOUD_CODER_BRIDGE=f'http://127.0.0.1:{server.server_port}',
                   HETEROCLOUD_CODER_ACCOUNT_ID=ACCOUNT, HETEROCLOUD_CODER_SERVICE_ID=SERVICE,
                   HETEROCLOUD_CODER_WAIT_SECONDS=str(seconds), HETEROCLOUD_CODER_POLL_SECONDS='1')
        try:
            result = subprocess.run(['/bin/sh', str(SCRIPT)], env=env, text=True,
                                    capture_output=True, timeout=seconds + 15)
            self.assertNotIn(SECRET, result.stdout + result.stderr)
            return result, server.seen
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

    def test_waits_for_current_revision_and_retries_a_transient_http_failure(self):
        result, requests = self.run_wait([None, service('ready', observed=3, ready=1),
                                          service('provisioning'), service('ready', ready=1)])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertGreaterEqual(requests, 4)
        self.assertIn('コンテナが起動しました', result.stdout)

    def test_replicated_storage_shortage_is_reported_instead_of_fake_running(self):
        result, requests = self.run_wait([service('provisioning', code='insufficient_replicated_storage')])
        self.assertNotEqual(result.returncode, 0)
        self.assertGreaterEqual(requests, 3)
        self.assertIn('永続ディスクを複製する空き容量が不足', result.stderr)

    def test_missing_registry_image_is_reported(self):
        result, _ = self.run_wait([service('provisioning', message='waiting for image inspection: artifact not found')])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('イメージが見つかりません', result.stderr)

    def test_temporary_capacity_shortage_does_not_fail_a_later_successful_start(self):
        result, requests = self.run_wait([
            service('provisioning', code='insufficient_compute_capacity'),
            service('provisioning', code='insufficient_compute_capacity'),
            service('provisioning', code='insufficient_compute_capacity'),
            service('provisioning', code='insufficient_compute_capacity'),
            service('ready', ready=1),
        ], seconds=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertGreaterEqual(requests, 5)
        self.assertIn('空き容量が不足', result.stdout)
        self.assertIn('コンテナが起動しました', result.stdout)

    def test_stale_ready_status_does_not_pass(self):
        result, _ = self.run_wait([service('ready', observed=3, ready=1)], seconds=2)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('起動を確認できません', result.stderr)

    def test_terraform_failure_and_retry_keep_the_same_service(self):
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.responses = [service('provisioning', code='insufficient_replicated_storage')]
        server.seen = 0
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory(prefix='coder-flash-terraform-') as tmp:
                config = {
                    'variable': {'running': {'type': 'bool', 'default': True}},
                    'resource': {'terraform_data': {
                        'service': {'input': 'existing-persistent-home'},
                        'workspace_ready': {
                            'count': '${var.running ? 1 : 0}',
                            'triggers_replace': ['${terraform_data.service.id}'],
                            'provisioner': [{'local-exec': {
                                'command': 'exec /bin/sh "$HETEROCLOUD_CODER_WAIT_SCRIPT"',
                                'environment': {
                                    'HETEROCLOUD_CODER_WAIT_SCRIPT': str(SCRIPT),
                                    'HETEROCLOUD_CODER_BRIDGE': f'http://127.0.0.1:{server.server_port}',
                                    'HETEROCLOUD_CODER_ACCOUNT_ID': ACCOUNT,
                                    'HETEROCLOUD_CODER_SERVICE_ID': SERVICE,
                                    'HETEROCLOUD_CODER_WAIT_SECONDS': '8',
                                    'HETEROCLOUD_CODER_POLL_SECONDS': '1',
                                },
                            }}],
                        },
                    }},
                }
                Path(tmp, 'main.tf.json').write_text(json.dumps(config))
                command = ['terraform', f'-chdir={tmp}']
                init = subprocess.run(command + ['init', '-backend=false', '-input=false', '-no-color'],
                                      text=True, capture_output=True, timeout=30)
                self.assertEqual(init.returncode, 0, init.stderr)

                def apply(*args):
                    return subprocess.run(command + ['apply', '-auto-approve', '-input=false', '-no-color', *args],
                                          text=True, capture_output=True, timeout=35)

                def service_id():
                    state = subprocess.run(command + ['show', '-json'], check=True,
                                           text=True, capture_output=True, timeout=10)
                    resources = json.loads(state.stdout)['values']['root_module']['resources']
                    return next(r['values']['id'] for r in resources if r['address'] == 'terraform_data.service')

                failed = apply()
                self.assertNotEqual(failed.returncode, 0)
                self.assertIn('永続ディスクを複製する空き容量が不足', failed.stderr + failed.stdout)
                original_id = service_id()
                server.responses = [service('ready', ready=1)]
                server.seen = 0
                recovered = apply()
                self.assertEqual(recovered.returncode, 0, recovered.stderr + recovered.stdout)
                self.assertEqual(service_id(), original_id)
                calls_before_stop = server.seen
                stopped = apply('-var=running=false')
                self.assertEqual(stopped.returncode, 0, stopped.stderr + stopped.stdout)
                self.assertEqual(service_id(), original_id)
                self.assertEqual(server.seen, calls_before_stop)
                self.assertNotIn(SECRET, failed.stderr + failed.stdout + recovered.stderr + recovered.stdout)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


if __name__ == '__main__':
    unittest.main()
