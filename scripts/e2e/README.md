# VPC live E2E

This test provisions real Flash workloads through the public API and CLI. It
uses two disposable tenants, never existing users' services. The parent receives
an expiring API key through Secret Manager and creates a private child with the
HeteroCloud API. No Docker socket or privileged tenant container is involved.

The administrator fixture helper requires `psycopg[binary]==3.2.10` and a mode
0600 database URL file. Database access only provisions/removes test identities
and scopes the parent's key to the test VPC's `children` group. It is never sent
to containers or stored in GitHub Actions secrets. All networking resources are
created, changed and removed using ordinary tenant APIs.

```sh
umask 077
python3 scripts/e2e/vpc_fixture.py create --dsn-file "$PRIVATE_DIR/database-url" --fixture "$PRIVATE_DIR/fixture.json"
python3 scripts/e2e/vpc_live.py --endpoint "$HETEROCLOUD_ENDPOINT" --cli heterocloud \
  --fixture "$PRIVATE_DIR/fixture.json" --dsn-file "$PRIVATE_DIR/database-url" \
  --report "$PRIVATE_DIR/report.json" --browser-output "$PRIVATE_DIR/browser"
python3 scripts/e2e/vpc_fixture.py remove --dsn-file "$PRIVATE_DIR/database-url" --fixture "$PRIVATE_DIR/fixture.json"
```

The report includes API/CLI lifecycle, cross-tenant denial, default-deny TCP/UDP,
private DNS, group and individual-service rules, wrong-port/group/VPC/tenant
denial, NAT enable/disable, rule revocation, no public child route, scoped parent
authorization, no automatic secret inheritance, and cleanup. DNS failures do
not count as successful policy-denial tests.

It also publishes the private child over HTTPS through the CLI with both NAT
and workload egress disabled. It verifies that private peer access still needs
an explicit VPC rule, that public and permitted private access work together,
and that withdrawing the public route preserves the permitted private path.
These checks use fresh unauthenticated requests to the external gateway. Route
withdrawal requires an HTTP 404/421; DNS failures and timeouts do not pass.

To exercise the documented nginx public example and the exact HTTPS endpoint
returned by the CLI, create a separate empty fixture and run:

```sh
python3 scripts/e2e/vpc_fixture.py create --dsn-file "$PRIVATE_DIR/database-url" --fixture "$PRIVATE_DIR/example-fixture.json"
python3 scripts/e2e/vpc_public_example.py --endpoint "$HETEROCLOUD_ENDPOINT" \
  --region "$HETEROCLOUD_REGION" --cli heterocloud \
  --fixture "$PRIVATE_DIR/example-fixture.json" --report "$PRIVATE_DIR/example-report.json"
python3 scripts/e2e/vpc_fixture.py remove --dsn-file "$PRIVATE_DIR/database-url" --fixture "$PRIVATE_DIR/example-fixture.json"
```

This runs `examples/cli/vpc.json` and `examples/cli/flash-vpc-public.json` with
fixture IDs and the selected region. It checks HTTPS access with NAT and egress
disabled, switches to private access, checks withdrawal using the same returned
URL, and removes its resources. Do not share this fixture with the full live
test running at the same time.

For debugging, `--keep-on-failure` retains only these disposable resources;
rerun with `--cleanup` before removing the fixture. Expired keys can be revoked
by removing the fixture after its services are deleted. The parent HTTP probe
only accepts fixed operations and predefined fixture destinations. It does not
expose a shell, a general proxy, or secret values.

NAT verification uses the deployment's `/cdn-cgi/trace` by default. For other
hosting arrangements, pass `--trace-url` with an owned HTTPS endpoint returning
the observed source address as `ip=...`. The product itself has no Cloudflare or
domain dependency.

The Cloud release workflow runs PostgreSQL, Rust, console and Chromium tests.
The VPC repository's CI also exercises the real kernel guard in isolated Linux
network namespaces, including route fallback, permit expiry, tunnel routing and
guard restart. The live test above requires an installed VPC/Flash deployment.

`--browser-output` runs the real console check before cleanup and automatically
creates its disposable browser session. Install the console's Playwright
dependencies and Chromium on the test host first. To inspect a retained test
fixture separately, before removing its networks, run:

```sh
python3 scripts/e2e/vpc_fixture.py browser-session \
  --dsn-file "$PRIVATE_DIR/database-url" --fixture "$PRIVATE_DIR/fixture.json"
node scripts/e2e/vpc_browser.mjs "$HETEROCLOUD_ENDPOINT" \
  "$PRIVATE_DIR/fixture.json" "$PRIVATE_DIR/browser"
```

This exercises the production API, desktop/mobile rendering, VPC data access,
JavaScript errors, horizontal overflow and the three-second page budget. Its
short-lived cookie belongs to a disposable test user; it does not test the
identity provider's interactive login. `remove` deletes this user and session.
Keep the fixture and screenshots private; publish only reviewed reports without
API keys, cookies, secret values or database URLs.
