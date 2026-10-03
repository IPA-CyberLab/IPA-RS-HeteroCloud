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
    parser.add_argument("action", choices=["create", "remove"])
    parser.add_argument("--dsn-file", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    args = parser.parse_args()
    with psycopg.connect(args.dsn_file.read_text().strip()) as db:
        if args.action == "remove":
            fixture = json.loads(args.fixture.read_text())
            for tenant in fixture["tenants"]:
                # Never cascade-delete live infrastructure or an unrelated tenant.
                count = db.execute("SELECT count(*) FROM service_instances WHERE organization_id=%s", (tenant["organization_id"],)).fetchone()[0]
                if count:
                    raise RuntimeError("Delete test services through the API before removing the fixture")
                db.execute("DELETE FROM organizations WHERE id=%s AND slug=%s AND slug LIKE 'vpc-e2e-%%'", (tenant["organization_id"], tenant["slug"]))
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
            document = {"version": "2026-07-31", "statements": [{"effect": "allow", "actions": ["*"], "resources": [f"hc:org:{org}:*"]}]}
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
