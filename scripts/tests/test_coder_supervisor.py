import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import signal
import subprocess
import tempfile
import threading
import time
import unittest


SUPERVISOR = Path(__file__).resolve().parents[2] / "examples/coder/code-server-supervisor.sh"


class SupervisorTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.home = self.root / "home"
        self.runtime = self.root / "runtime"
        self.home.mkdir()
        self.runtime.mkdir()
        self.binary = self.root / "ide"
        self.binary.write_text('#!/bin/sh\nprintf "started\\n" >> "$CODE_SERVER_RUNTIME_DIR/starts"\nexit 42\n')
        self.binary.chmod(0o700)
        self.processes = []

    def tearDown(self):
        for process in self.processes:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
        self.directory.cleanup()

    def launch(self, health_url="http://127.0.0.1:0/healthz"):
        env = {**os.environ, "HOME": str(self.home), "CODE_SERVER_RUNTIME_DIR": str(self.runtime), "CODE_SERVER_BIN": str(self.binary), "CODE_SERVER_HEALTH_URL": health_url}
        process = subprocess.Popen(["sh", str(SUPERVISOR)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        self.processes.append(process)
        return process

    def wait_starts(self, count):
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            file = self.runtime / "starts"
            if file.exists() and len(file.read_text().splitlines()) >= count:
                return
            time.sleep(0.05)
        self.fail(f"IDE did not start {count} times")

    def test_restarts_after_repeated_child_failures(self):
        process = self.launch()
        self.wait_starts(3)
        self.assertIsNone(process.poll())

    @unittest.skipUnless(Path("/dev/full").exists(), "Requires a real ENOSPC device")
    def test_enospc_diagnostic_write_does_not_kill_supervisor(self):
        for name in ("code-server.log", "supervisor.log"):
            (self.runtime / name).symlink_to("/dev/full")
        process = self.launch()
        self.wait_starts(3)
        self.assertIsNone(process.poll())

    def test_uses_container_runtime_when_home_projects_cannot_be_created(self):
        # ENOTDIR makes writes fail even when this check runs as root.
        self.home.rmdir()
        self.home.write_text("unwritable persistent-home stand-in")
        process = self.launch()
        self.wait_starts(2)
        self.assertIsNone(process.poll())
        self.assertEqual(self.home.read_text(), "unwritable persistent-home stand-in")

    def test_second_supervisor_does_not_start_another_ide(self):
        self.binary.write_text('#!/bin/sh\nprintf "started\\n" >> "$CODE_SERVER_RUNTIME_DIR/starts"\nexec sleep 60\n')
        first = self.launch()
        self.wait_starts(1)
        second = self.launch()
        self.assertEqual(second.wait(timeout=5), 0)
        self.assertIsNone(first.poll())
        self.assertEqual((self.runtime / "starts").read_text().splitlines(), ["started"])

    def test_adopts_a_healthy_existing_ide_then_recovers_its_exit(self):
        class Health(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()

            def log_message(self, *args):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Health)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            process = self.launch(f"http://127.0.0.1:{server.server_port}/healthz")
            time.sleep(2.5)
            self.assertIsNone(process.poll())
            self.assertFalse((self.runtime / "starts").exists())
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
        self.wait_starts(2)
        self.assertIsNone(process.poll())


if __name__ == "__main__":
    unittest.main()
