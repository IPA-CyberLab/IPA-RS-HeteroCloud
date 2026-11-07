# Flash workload log verification — 2026-11-07

The production API and Flash provider were deployed from the immutable releases
`0.1.114-platform.1` and `0.1.56-observability.2`. A real, private, stateless
Flash workload emitted a synthetic marker. The live test verified:

- A scoped service-account credential could list its replica and read the
  marker from the actual Pod log, with `Cache-Control: no-store`.
- Anonymous and other-organization requests were rejected. A nonexistent or
  unrelated Pod name returned 404, and the valid Pod still returned its log.
- The released Linux x64 CLI `0.1.114` read the same log.
- Desktop and mobile Chromium displayed the marker through the deployed UI and
  real API. These browsers used a disposable session; the test did not simulate
  interactive identity-provider login.
- The test service, two disposable organizations, test user, credentials,
  retained test revision/event history and verified test usage records were
  removed. No existing customer workload or disk was changed by this test.

The final test report is
`/workspace/.heteronetwork-iac/flash-logs-e2e-20261107/report-7.json` on the
operator workspace. The reusable test is `scripts/e2e/flash_logs_live.py`.
Provider and platform CI, including their unit and integration tests, passed;
all six CLI release builds passed on their corresponding Linux, macOS and
Windows x64/ARM runners. The independent VPN/browser acceptance workflow tests
the network and authenticated management console separately.
