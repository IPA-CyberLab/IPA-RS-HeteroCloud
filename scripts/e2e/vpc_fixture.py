#!/usr/bin/env python3
"""Administrator-only isolated live-test identities. Never prints credentials.

Requires psycopg[binary]. The data plane test uses ordinary scoped API keys;
database access is only used here to create/remove the disposable test tenants.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import uuid

import psycopg
from psycopg.types.json import Jsonb


def private_write(path, value):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as stream:
        stream.write(value)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["create", "remove", "grant-parent", "browser-session"])
    parser.add_argument("--dsn-file", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    parser.add_argument("--vpc-id")
    args = parser.parse_args()
    with psycopg.connect(args.dsn_file.read_text().strip()) as db:
        if args.action == "browser-session":
            fixture = json.loads(args.fixture.read_text())
            tenant = fixture["tenants"][0]
            assert tenant["slug"].startswith("vpc-e2e-")
            if "browser_session" in fixture:
                raise RuntimeError("Browser fixture already exists")
            user, principal, session = [str(uuid.uuid4()) for _ in range(3)]
            token = secrets.token_urlsafe(32)
            db.execute("INSERT INTO users(id,email,display_name,password_hash,status) VALUES (%s,%s,'VPC E2E','disabled-test-fixture','active')", (user, tenant["slug"] + "@example.invalid"))
            db.execute("INSERT INTO principals(id,organization_id,kind,name,user_id) VALUES (%s,%s,'user','VPC E2E browser',%s)", (principal, tenant["organization_id"], user))
            db.execute("INSERT INTO organization_memberships(organization_id,user_id,principal_id,role) VALUES (%s,%s,%s,'owner')", (tenant["organization_id"], user, principal))
            db.execute("INSERT INTO sessions(id,user_id,token_hash,expires_at) VALUES (%s,%s,%s,now()+interval '2 hours')", (session, user, hashlib.sha256(token.encode()).digest()))
            fixture["browser_session"] = token
            fixture["browser_user_id"] = user
            args.fixture.write_text(json.dumps(fixture))
            print("Disposable browser session created; no identity-provider login is simulated")
            return
        if args.action == "grant-parent":
            fixture = json.loads(args.fixture.read_text())
            tenant = fixture["tenants"][0]
            org = tenant["organization_id"]
            owned = db.execute("SELECT id FROM service_instances WHERE id=%s AND organization_id=%s AND provider='vpc'", (args.vpc_id, org)).fetchone()
            if not owned or not tenant["slug"].startswith("vpc-e2e-"):
                raise RuntimeError("Parent scope requires this fixture's own VPC")
            if "parent_api_key" in tenant:
                raise RuntimeError("Parent credential already exists; refusing to broaden it")
            principal, policy = str(uuid.uuid4()), str(uuid.uuid4())
            prefix = secrets.token_hex(8)
            token = f"hc_{prefix}_{secrets.token_urlsafe(32)}"
            document = {"version": "2026-07-31", "statements": [
                {"effect": "Allow", "actions": ["flash:CreateInstance", "flash:ListInstances", "flash:GetInstance"], "resources": [f"hc:org:{org}:flash/*"]},
                {"effect": "Allow", "actions": ["vpc:AttachSecurityGroup"], "resources": [f"hc:org:{org}:vpc/network/{args.vpc_id}/security-group/children"]},
            ]}
            digest = hashlib.sha256((Path(__file__).resolve().parents[2] / "lean/HeteroCloud/IAM.lean").read_bytes()).hexdigest()
            db.execute("INSERT INTO principals(id,organization_id,kind,name) VALUES (%s,%s,'service_account','VPC E2E parent')", (principal, org))
            db.execute("INSERT INTO iam_policies(id,organization_id,name,document,semantics_digest) VALUES (%s,%s,'VPC E2E parent',%s,%s)", (policy, org, Jsonb(document), digest))
            db.execute("INSERT INTO iam_bindings(id,organization_id,principal_id,policy_id) VALUES (%s,%s,%s,%s)", (str(uuid.uuid4()), org, principal, policy))
            db.execute("INSERT INTO api_keys(id,organization_id,principal_id,name,prefix,secret_hash,expires_at) VALUES (%s,%s,%s,'VPC E2E parent',%s,%s,now()+interval '8 hours')", (str(uuid.uuid4()), org, principal, prefix, hashlib.sha256(token.encode()).digest()))
            tenant["parent_api_key"] = token
            args.fixture.write_text(json.dumps(fixture))
            print("Parent key is restricted to Flash creation and the children security group")
            return
        if args.action == "remove":
            fixture = json.loads(args.fixture.read_text())
            for tenant in fixture["tenants"]:
                # Never cascade-delete live infrastructure or an unrelated tenant.
                count = db.execute("SELECT count(*) FROM service_instances WHERE organization_id=%s", (tenant["organization_id"],)).fetchone()[0]
                if count:
                    raise RuntimeError("Delete test services through the API before removing the fixture")
                db.execute("DELETE FROM organizations WHERE id=%s AND slug=%s AND slug LIKE 'vpc-e2e-%%'", (tenant["organization_id"], tenant["slug"]))
            if fixture.get("browser_user_id"):
                db.execute("DELETE FROM users WHERE id=%s AND email=%s", (fixture["browser_user_id"], fixture["tenants"][0]["slug"] + "@example.invalid"))
            print("Disposable VPC test identities removed")
            return
        if args.fixture.exists():
            raise RuntimeError("Fixture already exists; refusing to overwrite its cleanup records")
        fixture = {"schema_version": 1, "tenants": []}
        digest = hashlib.sha256((Path(__file__).resolve().parents[2] / "lean/HeteroCloud/IAM.lean").read_bytes()).hexdigest()
        for _ in range(2):
            org, project, principal, policy, key_id = [str(uuid.uuid4()) for _ in range(5)]
            slug = "vpc-e2e-" + secrets.token_hex(6)
            prefix = secrets.token_hex(8)
            token = f"hc_{prefix}_{secrets.token_urlsafe(32)}"
            document = {"version": "2026-07-31", "statements": [{"effect": "Allow", "actions": ["*"], "resources": [f"hc:org:{org}:*"]}]}
            db.execute("INSERT INTO organizations(id,slug,name) VALUES (%s,%s,%s)", (org, slug, slug))
            db.execute("INSERT INTO projects(id,organization_id,slug,name) VALUES (%s,%s,'vpc-test','Disposable VPC test')", (project, org))
            db.execute("INSERT INTO principals(id,organization_id,kind,name) VALUES (%s,%s,'service_account','VPC E2E driver')", (principal, org))
            db.execute("INSERT INTO iam_policies(id,organization_id,name,document,semantics_digest) VALUES (%s,%s,'VPC E2E driver',%s,%s)", (policy, org, Jsonb(document), digest))
            db.execute("INSERT INTO iam_bindings(id,organization_id,principal_id,policy_id) VALUES (%s,%s,%s,%s)", (str(uuid.uuid4()), org, principal, policy))
            db.execute("INSERT INTO api_keys(id,organization_id,principal_id,name,prefix,secret_hash,expires_at) VALUES (%s,%s,%s,'VPC E2E driver',%s,%s,now()+interval '8 hours')", (key_id, org, principal, prefix, hashlib.sha256(token.encode()).digest()))
            fixture["tenants"].append({"organization_id": org, "project_id": project, "slug": slug, "api_key": token})
        # Write before commit: a failed commit leaves recoverable local records,
        # whereas a successful commit can never orphan unknown credentials.
        private_write(args.fixture, json.dumps(fixture))
    print("Two disposable VPC test tenants created; credentials saved with mode 0600")


if __name__ == "__main__":
    main()
