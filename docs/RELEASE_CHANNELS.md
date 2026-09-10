# Channel Artifact Publishing

The release workflow attaches `heterocloud-release-artifact.json` containing
`schema_version: 1`, `component: "heterocloud"`, the release version without
leading `v`, the full 40-character commit, and the pushed
`ghcr.io/ipa-cyberlab/ipa-rs-heterocloud@sha256:...` image reference.
No example digest is a deployable artifact.

Before publication, the exact fully qualified release tag must resolve to both
the checked-out HEAD and GitHub release-event SHA. Tag syntax follows
HeteroNetwork's channel grammar; `+build` metadata is rejected before pushing.
Existing Rust, database, console (where present), and Helm gates remain.

The artifact binds the amd64 API/worker image; CLI archives and their checksum
files remain separate release assets. It does not attest CLI matrix completion.

Dev releases such as `v0.1.71-dev.1` need not equal the source Cargo version.
Neither Dockerfile injects a release version into Rust compilation: binaries
retain their source Cargo version. OCI labels record the release version,
exact revision, and `io.heterocloud.cargo-version` separately. No source manifests
are rewritten. `latest` is updated only for a numeric stable version when the
GitHub release is explicitly not a prerelease.

Release asset uploads never overwrite existing assets. An existing channel
artifact rejects a repeat full run before building. A partially failed release
requires operator inspection; the workflow does not delete assets or silently
replace them. These checks do not make mutable registry tags immutable.

Publication does not select dev/prod, deploy workloads, or prove successful dev
testing. Stage the actual JSON using HeteroNetwork's existing channel tool;
review isolated-dev evidence before promoting the exact same artifact.
This JSON is not a publisher signature.

Offline focused tests (Python 3.11+):
`python3 -m unittest discover -s scripts/tests -p test_release_artifact.py -v`
