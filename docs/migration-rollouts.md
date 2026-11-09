# Staged migration 36 rollout

SQLx normally rejects a database that contains a migration unknown to the
binary. Every API, owner-console and worker process runs migrations at startup.
Activating a migration before replacing all old binaries therefore prevents an
old replica from restarting and prevents a rollback to an older release.

Migration 36 only adds Flash rate and budget tables. The compatibility release
embeds that exact migration but caps execution at 35. First deploy it to every
API, owner-console and worker replica, including idle and disaster-recovery
replicas. Verify the image digest of every running and schedulable replica and
that no older release can be restarted automatically. The compatibility
release is the rollback target after migration 36 is activated; a still older
release is not a safe rollback target. Keep database backups and verify the
compatibility release against a disposable database and restore before
proceeding.

Only a subsequent release may raise the execution cap to 36 and use those
tables. Its rollout must keep the compatibility image available. The
compatibility release still rejects unknown migration versions and checksum
changes to earlier migrations. Embedding migration 36 does not make arbitrary
future or destructive migrations safe: stage and validate each separately.
