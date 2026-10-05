#!/usr/bin/env python3
"""Real VPC workspace stop/start and persistent-home test using an empty disposable fixture.

Requires websockets and the vpc_fixture.py fixture. API keys remain in private
files; reports contain checks and resource IDs, never credentials or exec output.
"""
import argparse
import asyncio
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import time
import urllib.parse

from websockets.asyncio.client import connect
from vpc_live import Api, IMAGE


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--endpoint", required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--cli", default="heterocloud")
    parser.add_argument("--region", default="heteronet-global")
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--browser-output", type=Path, help="Test real desktop/mobile Chromium using the fixture browser session")
    parser.add_argument("--cleanup", action="store_true")
    args = parser.parse_args()
    fixture = json.loads(args.fixture.read_text())
    tenant = fixture["tenants"][0]
    assert tenant["slug"].startswith("vpc-e2e-")
    api = Api(args.endpoint, tenant)
    report = json.loads(args.report.read_text()) if args.cleanup else {
        "schema_version": 1, "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "checks": [], "resources": [], "passed": False,
    }

    def save():
        args.report.write_text(json.dumps(report, indent=2)+"\n")

    def record(name, **details):
        report["checks"].append({"name": name, "passed": True, **details})
        save()
        print("PASS "+name, flush=True)

    def cleanup():
        for resource in reversed(report["resources"]):
            path = resource["path"]
            value = api.call("GET", path, expected=[200,404])
            if value.get("error",{}).get("code") == "not_found":
                continue
            assert value["organization_id"] == tenant["organization_id"]
            assert value["name"].startswith("flash-lifecycle-")
            if path.startswith("flash/"):
                if value["spec"].get("secret_env"):
                    spec = {**value["spec"], "secret_env":{}, "secret_files":{}}
                    api.call("PUT",path,{"name":value["name"],"spec":spec})
                    api.wait(path)
                for name in api.call("GET",path+"/secrets")["items"]:
                    api.call("DELETE",path+"/secrets/"+name)
            api.call("DELETE",path)
            api.wait(path,deleted=True)
        record("disposable_services_and_vpc_deleted")

    if args.cleanup:
        cleanup()
        return
    assert not args.report.exists(), "refusing to overwrite cleanup records"
    assert not api.call("GET","flash/services")["items"], "fixture must be empty"
    assert not api.call("GET","vpc/networks")["items"], "fixture must be empty"
    nonce = secrets.token_hex(6)
    try:
        vpc = api.call("POST","vpc/networks",{
            "project_id":tenant["project_id"], "name":"flash-lifecycle-vpc-"+nonce,
            "spec":{"region":args.region,"description":"Disposable lifecycle test",
                "nat":{"enabled":False},"security_groups":["workspace"],"rules":[]},
        })
        report["resources"].append({"path":"vpc/networks/"+vpc["id"]}); save()
        api.wait("vpc/networks/"+vpc["id"])
        service = api.call("POST","flash/services",{
            "project_id":tenant["project_id"],"name":"flash-lifecycle-workspace-"+nonce,
            "spec":{"region":args.region,"image":IMAGE,"replicas":1,"cpu_millis":100,
                "memory_mib":128,"ephemeral_storage_gib":1,
                "ports":[{"name":"http","protocol":"tcp","container_port":8080}],
                "exposure":{"type":"internal","traffic_mode":"forwarded"},
                "egress":{"mode":"disabled"},
                "network":{"vpc_id":vpc["id"],"security_groups":["workspace"],"private_name":"workspace"},
                "env":{"HOME":"/root"},"command":["python3"],"args":["-m","http.server","8080"],
                "metadata":{"test":"flash-lifecycle"}},
        })
        path = "flash/services/"+service["id"]
        report["resources"].append({"path":path}); save()
        service = api.wait(path)
        api.call("PUT",path+"/secrets/lifecycle-token",{"value":"fixture-"+nonce})
        api.call("PUT",path,{"name":service["name"],"spec":{**service["spec"],"secret_env":{"LIFECYCLE_TOKEN":"lifecycle-token"}}})
        original = api.wait(path)
        assert original["spec"]["exposure"]["type"] == "internal"
        record("private_vpc_workspace_ready", service_id=service["id"])

        async def exec_marker(command, expected):
            containers = api.call("GET",path+"/containers")["items"]
            pod = next(x["name"] for x in containers if x["ready"])
            url = args.endpoint.rstrip("/").replace("https:","wss:").replace("http:","ws:")+"/api/v1/organizations/"+tenant["organization_id"]+"/"+path+"/exec?"+urllib.parse.urlencode({"pod":pod})
            async with connect(url, additional_headers={"Authorization":"Bearer "+tenant["api_key"]},
                    origin=args.endpoint.rstrip("/"), proxy=None, open_timeout=20) as ws:
                await ws.send((command+"; exit\r").encode())
                data=b""
                async with asyncio.timeout(25):
                    while expected.encode() not in data:
                        piece=await ws.recv(); data+=piece.encode() if isinstance(piece,str) else piece
            return pod

        # Construct the marker at runtime; terminal echo cannot satisfy this check.
        marker = "PERSIST_"+nonce+"_OK"
        old_pod = asyncio.run(exec_marker("printf 'PERSIST_%s_OK' '"+nonce+"' > /root/lifecycle-marker; test \"$LIFECYCLE_TOKEN\" = 'fixture-"+nonce+"' && cat /root/lifecycle-marker",marker))
        record("persistent_file_written_and_secret_injected")
        with tempfile.TemporaryDirectory(prefix="hc-lifecycle-key-") as private:
            keyfile=Path(private)/"api-key"; keyfile.write_text(tenant["api_key"]); keyfile.chmod(0o600)
            env=os.environ.copy(); env.pop("HETEROCLOUD_API_KEY",None)
            def cli(action):
                result=subprocess.run([args.cli,"--endpoint",args.endpoint,"--organization-id",tenant["organization_id"],
                    "--api-key-file",str(keyfile),"--wait-timeout-seconds","600","flash",action,service["id"]],
                    env=env,capture_output=True,text=True,timeout=650)
                assert result.returncode == 0, "CLI "+action+" failed (output withheld)"
                return json.loads(result.stdout)
            stopped=cli("stop")
            inner=stopped["status"].get("status",stopped["status"])
            assert inner["stopped"] and inner["ready_replicas"]==0 and inner["desired_replicas"]==0
            assert api.call("GET",path+"/containers")["items"]==[]
            assert api.call("GET",path+"/secrets")["items"]==["lifecycle-token"]
            normalized={k:v for k,v in stopped["spec"].items() if k!="stopped"}
            assert normalized==original["spec"]
            record("cli_stop_drains_all_containers_and_retains_settings_and_secret", generation=stopped["generation"])
            repeated=api.call("POST",path+"/stop")
            assert repeated["generation"]==stopped["generation"]
            time.sleep(10)
            assert api.call("GET",path+"/containers")["items"]==[]
            record("repeated_stop_is_inert_and_no_automatic_restart")
            resumed=cli("start")
            assert resumed["id"]==original["id"] and resumed["spec"]==original["spec"]
            inner=resumed["status"].get("status",resumed["status"])
            assert not inner["stopped"] and inner["ready_replicas"]==1
            new_pod=asyncio.run(exec_marker("test \"$LIFECYCLE_TOKEN\" = 'fixture-"+nonce+"' && cat /root/lifecycle-marker",marker))
            assert new_pod!=old_pod
            record("cli_resume_creates_new_container_with_same_home_and_secret", generation=resumed["generation"])
            repeated=api.call("POST",path+"/start")
            assert repeated["generation"]==resumed["generation"]
            record("repeated_start_is_inert")
        if args.browser_output:
            subprocess.run(["node", str(Path(__file__).with_name("flash_lifecycle_browser.mjs")),
                args.endpoint, str(args.fixture), service["id"], str(args.browser_output)], check=True, timeout=700)
            browser_report=json.loads((args.browser_output/"browser-report.json").read_text())
            assert browser_report["passed"]
            record("real_console_desktop_and_mobile_stop_start", records=browser_report["records"])
            asyncio.run(exec_marker("test \"$LIFECYCLE_TOKEN\" = 'fixture-"+nonce+"' && cat /root/lifecycle-marker",marker))
            record("persistent_file_and_secret_retained_after_console_cycles")
        report["passed"]=True
    finally:
        try:
            cleanup()
        except Exception:
            report["passed"]=False
            report["cleanup_failed"]=True
            raise
        finally:
            report["finished_at"]=time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime())
            save()
    print("PASS flash lifecycle live E2E",flush=True)


if __name__ == "__main__":
    main()
