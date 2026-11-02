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

The operator quota view groups accounts by cloud organization. Migration 0029
adds an initially disabled gate for aggregate **service allocation** limits:
Flow service/room allocations, Flash service/replica/CPU/memory/disk allocations,
and Syouyu bucket allocations. Once every API replica uses the new enforcement,
the management account's operator-set quota is also the organization's ceiling.
Requests across accounts use one transaction lock; deleting services remain
charged until provider deletion completes. Existing account and member limits
continue to apply. While the gate is disabled, limits remain account-scoped.

Migration 0030 provides a separate, initially disabled gate for Flash weekly
CPU/memory/GPU usage across accounts. It requires an upgraded Flash provider and
CRD as well as upgraded workers; it must not be enabled during a mixed-version
rollout. Existing usage records, including those for deleted services, count
toward the organization ceiling. After activation or an account/limit change,
the worker reconciles affected provider policies asynchronously; enforcement
follows the normal Flash metering and reconciliation interval. Account and
member weekly limits remain in force.

An account administrator's quota page shows both the account's own limits and
the shared organization allocation and weekly limits, with the enforcement
state of each shared limit. Organization-wide Registry storage is not yet
enforced; its account-specific limits are shown separately. Ordinary members
see only their own allocation rather than other members' quota details.

Identity policies, permissions boundaries, SCPs, and role session policies
support AWS date comparisons on the always-present `aws:CurrentTime` key:
`DateEquals`, `DateNotEquals`, `DateLessThan`, `DateLessThanEquals`,
`DateGreaterThan`, and `DateGreaterThanEquals`. Values may be RFC 3339 timestamps
or whole Unix seconds. A request uses one captured timestamp throughout all
layers. Other condition keys and operators are rejected; in particular client
IP headers are not trusted as a source for `aws:SourceIp`.

Registry storage and direct provider credential usage remain account-scoped.
Do not assume those are organization-wide.

IAM roles have a restricted AWS-style trust policy and short-lived HeteroCloud
bearer sessions. Cross-account assumption requires both a source identity
permission and a matching target trust policy. Optional session policies only
reduce permissions. Role sessions are time-limited, revocable, and cannot be
turned into long-lived API keys or used as Flash task identities to bypass the
trust policy. The console can issue a bearer token once for API use or switch
the browser to the role. Browser role sessions are bound to the existing browser
session in the database; the role token is not exposed to JavaScript. A revoked,
expired or disabled browser role fails closed until the user explicitly exits
the role, rather than silently reverting to the original identity.
The account menu accepts a destination account number and role name, including
roles in accounts where the user has no direct membership or IAM list access.

## Deployment and activation

The migration intentionally leaves `cloud_security_activation.scp_enabled`
false. While false, the API refuses SCP, group, and permissions-boundary changes. Migration 0027
adds a separate `roles_enabled` flag, initially false, which blocks role
management and assumption. Migration 0028 adds `browser_roles_enabled`, initially
false, which blocks browser role switching until every API replica understands
the switched identity. These gates prevent a new
replica from admitting a policy that an old replica would not enforce. After
every API replica is ready on the new immutable image, an operator enables the
controls once using a transactional DB migration/job:

```sql
UPDATE cloud_security_activation SET scp_enabled = true, roles_enabled = true, browser_roles_enabled = true WHERE singleton = true;
```

After all API replicas have also been upgraded to migration 0029's service
allocation enforcement, verify their actual images and endpoints and activate
the separate `organization_service_quotas_enabled` gate. This gate must not be
enabled during a mixed-version rollout.

Only after the Flash provider CRD, every Flash API/controller replica, and every
HeteroCloud worker have been verified on compatible images, a cluster operator
may run `heterocloud-worker --enable-organization-flash-weekly-quotas` in a
worker container. The command atomically sets the weekly gate and enqueues a
policy refresh for every Flash service; repeat it safely to retry a refresh.
Confirm the refresh queue drains and each provider policy includes the correct
account set before relying on the organization-wide weekly ceiling.

Migration 0031 keeps `iam_conditions_enabled` false until all API instances
understand time conditions. After verifying the actual running API and worker
images and endpoints, the operator may enable that gate. Never roll back to a
binary that cannot evaluate a policy condition after accepting one.

Migration 0032 keeps `organization_registry_credentials_enabled` false until
all API instances enforce the shared Registry credential reservation. After
verifying every API and worker image and endpoint, run
`heterocloud-worker --enable-organization-registry-credentials` in a worker
container. Activation waits for in-flight quota reservations and is idempotent.

Verify the deployment's actual image digests and readiness, rather than only
the desired image, before activation. Once activated, do not roll back to a
binary without this migration and enforcement: old binaries also cannot start
against the newer SQLx migration set. Roll forward to a compatible build. The
activation applies to the management API; already issued provider/data-plane
credentials have their own authorization and revocation mechanisms.
Role and workload sessions cannot issue Registry or Syouyu credentials, or
issue or rotate Flow developer credentials: these credentials outlive the
temporary session. This restriction does not revoke previously issued secrets.
Flow developer credentials are checked against the issuer's current IAM policy,
permissions boundary and inherited SCP whenever they are used. Credentials
previously issued by an assumed role cannot be used because their original
session policy and expiry cannot be reconstructed. A previously issued Flow
access context remains usable for at most five minutes unless revoked; Registry
and Syouyu data-plane requests still do not evaluate these policies directly.

## Current compatibility boundaries

This is not complete AWS Organizations or IAM compatibility. It does not yet
implement IAM Identity Center and permission sets, account invitations between
organizations, AWS root credentials, federated or service principals in role
trust policies, general resource-based and cross-account policies, condition
operators or keys beyond the time subset above, general AWS service ARNs, AWS SDK/CLI APIs, consolidated billing,
or direct enforcement on previously issued data-plane credentials. Unsupported
policy fields are rejected. AWS resource and action names cannot be assumed to
map to HeteroCloud resources. Provider-specific controls must remain in place.
Role trust supports AWS account IDs, root ARNs and explicit HeteroCloud IAM
user/role ARNs; HeteroCloud user ARNs use the stable principal UUID. It always
requires an identity-based `sts:AssumeRole` allow, including for same-account
callers, which is stricter than some AWS resource-policy grants. The token is a
HeteroCloud bearer token, not an AWS STS access-key/secret/session-token tuple.

AWS describes the source semantics in its [Organizations concepts](https://docs.aws.amazon.com/organizations/latest/userguide/orgs_getting-started_concepts.html),
[SCP documentation](https://docs.aws.amazon.com/organizations/latest/userguide/orgs_manage_policies_scps.html),
[IAM policy evaluation documentation](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_evaluation-logic.html),
and [condition operator reference](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_elements_condition_operators.html).
