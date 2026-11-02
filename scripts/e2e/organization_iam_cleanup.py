#!/usr/bin/env python3
"""Remove only the test account recorded by organization_iam_browser.mjs."""
import argparse
import json
from pathlib import Path
import psycopg


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dsn-file", required=True, type=Path)
    parser.add_argument("--fixture", required=True, type=Path)
    args = parser.parse_args()
    fixture = json.loads(args.fixture.read_text())
    tenant = fixture["tenants"][0]
    slug = fixture.get("iam_child_slug")
    if not slug:
        print("No test member account to remove")
        return
    if not tenant["slug"].startswith("vpc-e2e-") or not slug.startswith("vpc-e2e-child-"):
        raise RuntimeError("refusing to remove a non-test account")
    with psycopg.connect(args.dsn_file.read_text().strip()) as db:
        row = db.execute("""SELECT child.account_id FROM cloud_organization_accounts child
            JOIN cloud_organization_accounts parent ON parent.organization_id=child.organization_id
            JOIN organizations account ON account.id=child.account_id
            WHERE parent.account_id=%s AND account.slug=%s AND child.account_id<>parent.account_id
            FOR UPDATE OF child""", (tenant["organization_id"], slug)).fetchone()
        if not row:
            print("No test member account remains")
            return
        child = str(row[0])
        if fixture.get("iam_child_id") and fixture["iam_child_id"] != child:
            raise RuntimeError("recorded test account identity does not match")
        count = db.execute("SELECT count(*) FROM service_instances WHERE organization_id=%s", (child,)).fetchone()[0]
        if count:
            raise RuntimeError("remove test services through the API before deleting the account")
        deleted = db.execute("DELETE FROM organizations WHERE id=%s AND slug=%s", (child, slug)).rowcount
        if deleted != 1:
            raise RuntimeError("test member account was not deleted")
    print("Disposable organization member account removed")


if __name__ == "__main__":
    main()
