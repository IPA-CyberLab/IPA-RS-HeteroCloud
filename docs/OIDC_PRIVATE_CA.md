# OIDC Private CA

For an identity provider signed by a private CA, configure the API with
`--oidc-root-ca-file=/path/to/ca.crt` or
`HETEROCLOUD_OIDC_ROOT_CA_FILE`. The PEM bundle augments the OIDC HTTP client's
normal trust roots only. Certificate signatures, validity and hostname checks
remain enabled. Other provider clients and database connections are unaffected.

With the Helm chart, provision a Secret in the release namespace containing only
the public CA bundle, then configure:

```yaml
oidc:
  enabled: true
  rootCaSecretName: heterocloud-dev-identity-ca
  rootCaSecretKey: ca.crt
```

Keep the existing issuer, client credentials and callback settings. The chart
mounts this Secret read-only into both the API and enabled owner console, not the
worker. It does not set process-wide `SSL_CERT_FILE` or disable TLS verification.
Never put a CA private key in this Secret.

The bundle is loaded at process startup: replacing Secret contents requires a
rolling restart of the API and owner console. Secret updates alone do not reload
the HTTP client. Missing, empty, oversized (over 256 KiB), private-key-containing,
or invalid bundles prevent startup; at most 16 certificates are accepted.

Focused coverage uses a local HTTPS discovery server and synthetic test-only
certificates to check explicit trust and rejection without that trust. Chart tests
verify both consumers, the selected Secret key and unchanged worker configuration.
These checks do not prove deployment readiness, real user login, callback handling
or identity-provider failover; those require the isolated DEV environment.
