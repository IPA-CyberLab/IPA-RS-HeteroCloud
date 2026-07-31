# HeteroCloud IAM Lean kernel

`HeteroCloud.IAM.authorize` is the final authorization decision shared with
the Rust evaluator. Lean proves the three non-negotiable invariants:

- cross-organization requests are denied;
- an applicable explicit deny overrides every allow;
- requests without an applicable allow are denied.

The API records the SHA-256 digest of this source with every admitted policy.
Release validation builds this package and runs the Rust truth-table tests.
