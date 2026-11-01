# AWS-style account and IAM model

The previous `organizations` database rows remain the isolated **accounts**.
Their UUIDs, users, IAM bindings, service ownership and quotas are unchanged.
Migration 0026 assigns each account a distinct 12-digit number and creates a
separate organization with that account as its management account and an
organization root. Existing accounts are not automatically merged. Users can
create member accounts and OUs under a management account and switch between
accounts in which they have a membership. Newly created member accounts give
their creator an account administrator membership.

IAM user groups can have users and identity policies. An optional permissions
boundary limits identity-based permissions for a user or service account. An explicit deny in a
direct or group policy also applies to an account administrator. Member-account
SCPs are evaluated at every ancestor OU, root, and account: every level must
allow the request, and any explicit deny wins. Each entity initially has the
managed `FullAWSAccess` policy, and the last attachment cannot be removed. SCPs
do not apply to the management account. Existing HeteroCloud quota ownership
and per-user quota extensions remain in force per account.

## Deployment and activation

The migration intentionally leaves `cloud_security_activation.scp_enabled`
false. While false, the API refuses SCP, group, and permissions-boundary changes. This prevents a new
replica from admitting a policy that an old replica would not enforce. After
every API replica is ready on the new immutable image, an operator enables the
controls once using a transactional DB migration/job:

```sql
UPDATE cloud_security_activation SET scp_enabled = true WHERE singleton = true;
```

Verify the deployment's actual image digests and readiness, rather than only
the desired image, before activation. Once activated, do not roll back to a
binary without this migration and enforcement: old binaries also cannot start
against the newer SQLx migration set. Roll forward to a compatible build. The
activation applies to the management API; already issued provider/data-plane
credentials have their own authorization and revocation mechanisms.

## Current compatibility boundaries

This is not complete AWS Organizations or IAM compatibility. It does not yet
implement IAM Identity Center and permission sets, account invitations between
organizations, AWS root credentials, IAM role trust policies and STS, IAM
session policies, resource-based and cross-account
policies, policy conditions, AWS ARNs, AWS SDK/CLI APIs, consolidated billing,
or direct enforcement on previously issued data-plane credentials. Unsupported
policy fields are rejected. AWS resource and action names cannot be assumed to
map to HeteroCloud resources. Provider-specific controls must remain in place.

AWS describes the source semantics in its [Organizations concepts](https://docs.aws.amazon.com/organizations/latest/userguide/orgs_getting-started_concepts.html),
[SCP documentation](https://docs.aws.amazon.com/organizations/latest/userguide/orgs_manage_policies_scps.html),
and [IAM policy evaluation documentation](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_evaluation-logic.html).
