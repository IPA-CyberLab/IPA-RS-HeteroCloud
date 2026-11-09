# Stable CLI and image releases

Create a GitHub prerelease whose tag is the final stable version, for example
`v0.1.116`. A prerelease with a plain version tag builds the image and all six
CLI targets; `-platform.N` prereleases build only the image. The prerelease is
not selected by the stable CLI updater.

Wait for the `Release` workflow for that exact tag to succeed. Run the
HeteroNetwork `HeteroCloud budget image upgrade E2E` workflow against the
published image when the release changes the budget/database migration. Its
database and Flash provider are disposable fixtures; it is not a production
availability test.

Dispatch `Promote verified stable release` on `master` with the tag and the
successful `Release` run ID. First set `dry_run` to true. This checks the
commit, successful release workflow, immutable image artifact, all six native
CLI archives and their SHA-256 checksums. Dispatch again with `dry_run` false
only after those checks and the relevant image E2E succeed. The workflow copies
the verified image digest to the container `latest` tag, verifies that digest,
then marks the GitHub release stable and latest. It refuses to replace a newer
stable version. The two publications are not an atomic transaction; if the
final GitHub update fails, rerun the promotion and verify both destinations.
