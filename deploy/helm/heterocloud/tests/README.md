# Chart Render Checks

Prerequisites: Helm 3, Python 3, and the test dependencies below.

```sh
python3 -m venv /tmp/heterocloud-chart-tests
/tmp/heterocloud-chart-tests/bin/pip install -r deploy/helm/heterocloud/tests/requirements.txt
/tmp/heterocloud-chart-tests/bin/python -B -m unittest discover -s deploy/helm/heterocloud/tests -p 'test_*.py'
helm lint deploy/helm/heterocloud
```

The top-level `hostAliases` value accepts a list of `{ip, hostnames}` objects.
Each IP must be an IPv4 or IPv6 address; each nonempty hostname list contains
lowercase DNS names. Both API and enabled owner-console pods receive the same
list. Worker pods, DNS policies, and other pod settings are unchanged.

The default is `[]`, which omits the pod field. Supply environment-specific
addresses through an external values override, not chart defaults. The checks
use documentation-only addresses and `helm template`; they do not contact a
cluster or deploy resources.

## Owner Console Cookies

`ownerConsole.secureCookie` is independent of the public API's top-level
`secureCookie`. Its default is `false` for the existing HTTP VPN owner origin.
When the owner console is enabled, HTTPS origins require `true`; HTTP origins
require `false`, matching the API's secure-cookie HTTPS requirement. Non-boolean
or scheme-mismatched configuration fails rendering instead of
silently emitting insecure cookies for an HTTPS owner console.

For HTTPS dev access, explicitly supply these values in the later deployment:

```yaml
ownerConsole:
  enabled: true
  origin: https://owner.dev.example.test
  secureCookie: true
  oidcPublicCallbackUrl: https://owner.dev.example.test/api/v1/auth/oidc/callback
```

The owner email, dev IdP client/secret, TLS edge, and allowed networks still need
their own configuration. This chart change does not modify production manifests
or establish deployment readiness. Tests cover HTTP compatibility, HTTPS refusal
and opt-in, strict value types, and unchanged public API/worker workloads.
