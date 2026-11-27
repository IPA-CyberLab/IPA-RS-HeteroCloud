"""Exercise the real Coder template and agent against the Flash capacity fixture."""

from contextlib import ExitStack
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


SOURCE = Path(__file__).resolve().parents[2] / "examples/coder"
WORKSPACE = "capacity-wait-fixture"


def request(url, body=None, token=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Coder-Session-Token"] = token
    data = None if body is None else json.dumps(body).encode()
    with urlopen(Request(url, data=data, headers=headers), timeout=5) as response:
        return json.load(response) if response.status != 204 else None


def wait_for(description, observe, *, timeout=90, process=None):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process is not None and process.poll() is not None:
            raise RuntimeError(f"{description}: fixture process exited ({process.returncode})")
        try:
            value = observe()
        except (URLError, ConnectionError, TimeoutError):
            value = None
        if value:
            return value
        time.sleep(1)
    raise RuntimeError(f"{description}: timed out")


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=8)


def run(root, processes):
    coder = os.environ["CAPACITY_FIXTURE_CODER"]
    bridge = os.environ["CAPACITY_FIXTURE_BRIDGE"]
    org = os.environ["CAPACITY_FIXTURE_ORG"]
    collection = f"{bridge}/api/v1/organizations/{org}/flash/services"
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        listen = f"127.0.0.1:{reservation.getsockname()[1]}"
    origin = f"http://{listen}"
    env = {
        **os.environ, "CODER_PG_CONNECTION_URL": os.environ["CAPACITY_FIXTURE_CODER_DB"],
        "CODER_HTTP_ADDRESS": listen, "CODER_ACCESS_URL": origin,
        "CODER_TELEMETRY_ENABLE": "false", "CODER_PROVISIONER_DAEMONS": "1",
        "CODER_DERP_SERVER_STUN_ADDRESSES": "disable",
        "CODER_CACHE_DIRECTORY": str(root / "cache"), "API_DATA_IS_SENSITIVE": "true",
    }

    def launch(name, *argv, runtime=None):
        log = processes.enter_context((root / f"{name}.log").open("wb"))
        process = subprocess.Popen([coder, *argv], cwd=root, env=runtime or env,
                                   stdout=log, stderr=subprocess.STDOUT)
        processes.callback(stop, process)
        return process

    server = launch("server", "server")
    wait_for("Coder API", lambda: request(f"{origin}/api/v2/buildinfo"), process=server)
    credentials = {"email": "fixture@example.test", "password": secrets.token_urlsafe(32)}
    request(f"{origin}/api/v2/users/first", {**credentials, "username": "fixture"})
    session = request(f"{origin}/api/v2/users/login", credentials)["session_token"]

    def cli(*argv):
        result = subprocess.run([coder, "--global-config", str(root / "client"), *argv],
                                cwd=root, env={**env, "CODER_URL": origin, "CODER_SESSION_TOKEN": session},
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
        if result.returncode:
            # Coder provisioner output can contain the sensitive Terraform
            # environment. Keep it in this fixture directory.
            (root / "cli-failed.log").write_bytes(result.stdout)
            raise RuntimeError(f"Coder {argv[0]} failed ({result.returncode})")

    template = root / "template"
    template.mkdir()
    for filename in ("main.tf", ".terraform.lock.hcl", "code-server-supervisor.sh", "wait-for-flash.sh"):
        shutil.copyfile(SOURCE / filename, template / filename)
    variables = {
        "provisioner_api_url": bridge, "organization_id": org,
        "project_id": os.environ["CAPACITY_FIXTURE_PROJECT"], "vpc_id": os.environ["CAPACITY_FIXTURE_VPC"],
        "workspace_image": "example.test/coder-workspace:v1", "coder_private_url": origin, "coder_url": origin,
    }
    cli("templates", "push", WORKSPACE, "--directory", str(template), "--yes",
        *[argument for k, v in variables.items() for argument in ("--variable", f"{k}={v}")])
    cli("create", WORKSPACE, "--template", WORKSPACE, "--yes", "--no-wait",
        "--parameter", "wait_for_flash_capacity=true")

    def workspace():
        return request(f"{origin}/api/v2/users/me/workspace/{WORKSPACE}", token=session)

    workspace_id = workspace()["id"]

    def service_waiting():
        build = workspace()["latest_build"]
        if build["status"] in ("failed", "canceled"):
            raise RuntimeError("Coder build ended while waiting for capacity")
        for item in request(collection)["items"]:
            if item["spec"]["metadata"].get("coder_workspace") != workspace_id:
                continue
            status = item["status"].get("capacity_wait", {})
            if status.get("reason") == "insufficient_compute_capacity":
                assert status["state"] == "waiting" and item["spec"]["stopped"]
                assert build["status"] != "running"
                return item
        return None

    original = wait_for("Flash capacity wait", service_waiting, process=server, timeout=120)
    request(f"{bridge}/fixture/release-capacity", {})

    def build_running():
        current = workspace()
        build = current["latest_build"]
        if build["status"] in ("failed", "canceled"):
            raise RuntimeError("Coder build failed after capacity was released")
        return current if build["status"] == "running" else None

    wait_for("Coder build after the release of capacity", build_running, process=server)
    active = request(f"{collection}/{original['id']}")
    assert active["status"]["capacity_wait"]["id"] == original["status"]["capacity_wait"]["id"]
    assert active["status"]["capacity_wait"]["state"] == "ready"
    assert active["generation"] == original["generation"] + 1
    token_file = root / "agent-token"
    token_file.write_text(active["spec"]["env"]["CODER_AGENT_TOKEN"])
    agent = launch("agent", "agent", runtime={
        **env, "CODER_AGENT_URL": origin, "CODER_AGENT_TOKEN_FILE": str(token_file),
    })
    wait_for("Coder workspace agent connection", lambda: any(
        agent["status"] == "connected"
        for resource in workspace()["latest_build"]["resources"]
        for agent in resource.get("agents", [])
    ), process=agent)
    print("Verified capacity wait, release, Flash generation and real Coder agent connection.", flush=True)


if __name__ == "__main__":
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix="coder-capacity-e2e-") as temporary, ExitStack() as stack:
        try:
            run(Path(temporary), stack)
        except HTTPError as error:
            raise SystemExit(f"Fixture HTTP request failed with {error.code}") from None
