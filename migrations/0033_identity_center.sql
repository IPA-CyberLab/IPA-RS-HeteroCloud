-- Never enable this until every API replica enforces the session and managed
-- role rules; otherwise an older replica could bypass a revoked assignment.
ALTER TABLE cloud_security_activation
    ADD COLUMN identity_center_enabled boolean NOT NULL DEFAULT false;

CREATE TABLE identity_center_permission_sets (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES cloud_organizations(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name ~ '^[A-Za-z0-9+=,.@_-]{1,32}$'),
    document jsonb NOT NULL,
    session_duration_seconds integer NOT NULL CHECK (session_duration_seconds BETWEEN 3600 AND 43200),
    deleted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, id)
);
CREATE UNIQUE INDEX identity_center_permission_sets_active_name
    ON identity_center_permission_sets (organization_id, lower(name)) WHERE deleted_at IS NULL;

CREATE TABLE identity_center_groups (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES cloud_organizations(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    deleted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, id)
);
CREATE UNIQUE INDEX identity_center_groups_active_name
    ON identity_center_groups (organization_id, lower(name)) WHERE deleted_at IS NULL;

CREATE TABLE identity_center_group_members (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    group_id uuid NOT NULL,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (organization_id, group_id) REFERENCES identity_center_groups(organization_id, id) ON DELETE CASCADE,
    UNIQUE (group_id, user_id)
);

CREATE TABLE identity_center_account_roles (
    organization_id uuid NOT NULL,
    permission_set_id uuid NOT NULL,
    account_id uuid NOT NULL,
    role_principal_id uuid NOT NULL UNIQUE REFERENCES iam_roles(principal_id) DEFERRABLE INITIALLY DEFERRED,
    policy_id uuid NOT NULL UNIQUE REFERENCES iam_policies(id) DEFERRABLE INITIALLY DEFERRED,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (permission_set_id, account_id),
    UNIQUE (organization_id, permission_set_id, account_id, role_principal_id),
    FOREIGN KEY (organization_id, permission_set_id) REFERENCES identity_center_permission_sets(organization_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, account_id) REFERENCES cloud_organization_accounts(organization_id, account_id) ON DELETE CASCADE
);

CREATE TABLE identity_center_assignments (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL,
    permission_set_id uuid NOT NULL,
    account_id uuid NOT NULL,
    role_principal_id uuid NOT NULL,
    subject_type text NOT NULL CHECK (subject_type IN ('user', 'group')),
    subject_id uuid NOT NULL,
    generation bigint NOT NULL DEFAULT 1 CHECK (generation > 0),
    revoked_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, permission_set_id, account_id, subject_type, subject_id),
    FOREIGN KEY (organization_id, permission_set_id, account_id, role_principal_id)
        REFERENCES identity_center_account_roles(organization_id, permission_set_id, account_id, role_principal_id) ON DELETE CASCADE
);

-- Membership identity changes when a user leaves and later rejoins. Principal
-- IDs alone cannot distinguish the old membership from a new one.
ALTER TABLE organization_memberships
    ADD COLUMN identity_center_membership_id uuid NOT NULL DEFAULT gen_random_uuid() UNIQUE;

ALTER TABLE iam_role_sessions
    ADD COLUMN identity_center_assignment_id uuid REFERENCES identity_center_assignments(id) ON DELETE CASCADE,
    ADD COLUMN identity_center_assignment_generation bigint,
    ADD COLUMN identity_center_user_id uuid REFERENCES users(id) ON DELETE CASCADE,
    ADD COLUMN identity_center_target_principal_id uuid REFERENCES principals(id) ON DELETE SET NULL,
    ADD COLUMN identity_center_source_membership_id uuid REFERENCES organization_memberships(identity_center_membership_id) ON DELETE SET NULL,
    ADD COLUMN identity_center_target_membership_id uuid REFERENCES organization_memberships(identity_center_membership_id) ON DELETE SET NULL,
    ADD COLUMN identity_center_group_membership_id uuid REFERENCES identity_center_group_members(id) ON DELETE SET NULL,
    ADD CONSTRAINT iam_role_sessions_identity_center_fields CHECK (
        (identity_center_assignment_id IS NULL AND identity_center_assignment_generation IS NULL
            AND identity_center_user_id IS NULL AND identity_center_target_principal_id IS NULL AND identity_center_group_membership_id IS NULL
            AND identity_center_source_membership_id IS NULL AND identity_center_target_membership_id IS NULL)
        OR (identity_center_assignment_id IS NOT NULL AND identity_center_assignment_generation IS NOT NULL
            AND identity_center_user_id IS NOT NULL)
    );

-- A removed assignment or group membership must not become valid again if an
-- administrator later adds it back. The immutable membership id and the
-- assignment generation captured by the session prevent such revival.
CREATE FUNCTION identity_center_session_is_active(session_id uuid) RETURNS boolean
LANGUAGE sql STABLE AS $$
    SELECT s.identity_center_assignment_id IS NULL OR EXISTS (
        SELECT 1 FROM identity_center_assignments a
        JOIN identity_center_permission_sets ps ON ps.id=a.permission_set_id AND ps.organization_id=a.organization_id
        JOIN cloud_organizations co ON co.id=a.organization_id
        JOIN users u ON u.id=s.identity_center_user_id AND u.status='active'
        JOIN organization_memberships m ON m.organization_id=co.management_account_id
            AND m.user_id=u.id AND m.principal_id=s.source_principal_id
            AND m.identity_center_membership_id=s.identity_center_source_membership_id
        JOIN principals source ON source.id=m.principal_id AND source.organization_id=m.organization_id AND source.enabled
        JOIN organization_memberships tm ON tm.organization_id=a.account_id AND tm.user_id=u.id
            AND tm.principal_id=s.identity_center_target_principal_id
            AND tm.identity_center_membership_id=s.identity_center_target_membership_id
        JOIN principals target ON target.id=tm.principal_id AND target.organization_id=tm.organization_id AND target.enabled
        WHERE a.id=s.identity_center_assignment_id AND a.revoked_at IS NULL
          AND a.generation=s.identity_center_assignment_generation AND ps.deleted_at IS NULL
          AND a.account_id=s.organization_id AND a.role_principal_id=s.role_principal_id
          AND s.source_organization_id=co.management_account_id
          AND ((a.subject_type='user' AND a.subject_id=u.id AND s.identity_center_group_membership_id IS NULL)
            OR (a.subject_type='group' AND EXISTS (
                SELECT 1 FROM identity_center_groups g JOIN identity_center_group_members gm
                    ON gm.group_id=g.id AND gm.organization_id=g.organization_id
                WHERE g.id=a.subject_id AND g.organization_id=a.organization_id AND g.deleted_at IS NULL
                    AND gm.id=s.identity_center_group_membership_id AND gm.user_id=u.id)))
    ) FROM iam_role_sessions s WHERE s.id=session_id
$$;
