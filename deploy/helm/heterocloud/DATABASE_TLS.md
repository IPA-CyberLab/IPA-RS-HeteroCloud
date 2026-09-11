# PostgreSQL CA Trust

The chart can supply a namespace-local PEM CA bundle to all SQLx database
clients, including background workers and migration Jobs where present:

```yaml
databaseTls:
  caSecretName: dev-postgres-ca
  caSecretKey: ca.crt
```

Create or reconcile this Secret separately in the workload namespace. Do not
copy production database credentials or CA private keys into DEV. Only the
public CA certificate bundle belongs in the referenced key. The Secret is not
optional: a missing Secret or key prevents container startup.

This adds `PGSSLROOTCERT` from a Secret reference; it does not rewrite or validate
the database URL. Provision the URL with `sslmode=verify-full` and a database
Service hostname covered by the server certificate. Do not provide a conflicting
`sslrootcert` URL option: SQLx URL options can override environment defaults.
Merely supplying a CA does not enforce TLS when the URL disables verification.

An empty `caSecretName` preserves the existing connection behavior. The setting
does not change HTTP, Redis, Garage, LiveKit, or TURN trust configuration.
Secret-backed environment values are captured at process startup. CA rotation
requires a controlled workload rollout with an overlapping trust bundle; it is
not automatically reloaded by this chart.

The focused template tests verify references, client coverage and invalid
configuration handling. They do not prove a TLS handshake or certificate
rejection against a running database.
