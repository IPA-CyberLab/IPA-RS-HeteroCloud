-- Existing organizations are already the isolation boundary for workloads,
-- quotas, credentials and provider calls. Preserve those UUIDs as accounts.
CREATE SEQUENCE cloud_account_number_seq AS bigint
    MINVALUE 100000000000 MAXVALUE 999999999999 START 100000000000 NO CYCLE;
ALTER TABLE organizations ADD COLUMN account_number bigint
    NOT NULL DEFAULT nextval('cloud_account_number_seq')
    CHECK (account_number BETWEEN 100000000000 AND 999999999999);
ALTER TABLE organizations ADD CONSTRAINT organizations_account_number_key UNIQUE (account_number);

-- Group memberships and group policy attachments are account-scoped. A user
-- can be a member of multiple groups; groups cannot contain other groups.
ALTER TABLE principals ADD CONSTRAINT principals_account_id_id_key UNIQUE (organization_id, id);
ALTER TABLE iam_policies ADD CONSTRAINT iam_policies_account_id_id_key UNIQUE (organization_id, id);
ALTER TABLE principals ADD COLUMN permissions_boundary_policy_id uuid;
ALTER TABLE principals ADD CONSTRAINT principals_permissions_boundary_same_account
    FOREIGN KEY (organization_id, permissions_boundary_policy_id) REFERENCES iam_policies(organization_id, id);
CREATE TABLE iam_groups (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, id), UNIQUE (organization_id, name)
);
CREATE TABLE iam_group_members (
    organization_id uuid NOT NULL,
    group_id uuid NOT NULL,
    principal_id uuid NOT NULL,
    PRIMARY KEY (group_id, principal_id),
    FOREIGN KEY (organization_id, group_id) REFERENCES iam_groups(organization_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, principal_id) REFERENCES principals(organization_id, id) ON DELETE CASCADE
);
CREATE TABLE iam_group_policy_attachments (
    organization_id uuid NOT NULL,
    group_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    PRIMARY KEY (group_id, policy_id),
    FOREIGN KEY (organization_id, group_id) REFERENCES iam_groups(organization_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, policy_id) REFERENCES iam_policies(organization_id, id) ON DELETE CASCADE
);

CREATE TABLE cloud_organizations (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    management_account_id uuid NOT NULL UNIQUE REFERENCES organizations(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE cloud_organizational_units (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES cloud_organizations(id) ON DELETE CASCADE,
    parent_id uuid,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, id),
    FOREIGN KEY (organization_id, parent_id)
        REFERENCES cloud_organizational_units(organization_id, id) ON DELETE CASCADE,
    CHECK (id <> parent_id)
);
CREATE UNIQUE INDEX cloud_organizations_one_root
    ON cloud_organizational_units (organization_id) WHERE parent_id IS NULL;
CREATE UNIQUE INDEX cloud_organizational_units_sibling_name
    ON cloud_organizational_units (organization_id, parent_id, lower(name)) WHERE parent_id IS NOT NULL;

CREATE TABLE cloud_organization_accounts (
    account_id uuid PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    organization_id uuid NOT NULL REFERENCES cloud_organizations(id) ON DELETE CASCADE,
    parent_id uuid NOT NULL,
    UNIQUE (organization_id, account_id),
    FOREIGN KEY (organization_id, parent_id)
        REFERENCES cloud_organizational_units(organization_id, id) ON DELETE CASCADE
);
CREATE INDEX cloud_organization_accounts_parent
    ON cloud_organization_accounts (organization_id, parent_id);

CREATE TABLE cloud_service_control_policies (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES cloud_organizations(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    document jsonb NOT NULL,
    is_managed boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (organization_id, id), UNIQUE (organization_id, name)
);
CREATE TABLE cloud_scp_unit_attachments (
    organization_id uuid NOT NULL,
    unit_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    PRIMARY KEY (unit_id, policy_id),
    FOREIGN KEY (organization_id, unit_id) REFERENCES cloud_organizational_units(organization_id, id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, policy_id) REFERENCES cloud_service_control_policies(organization_id, id) ON DELETE CASCADE
);
CREATE TABLE cloud_scp_account_attachments (
    organization_id uuid NOT NULL,
    account_id uuid NOT NULL,
    policy_id uuid NOT NULL,
    PRIMARY KEY (account_id, policy_id),
    FOREIGN KEY (organization_id, account_id) REFERENCES cloud_organization_accounts(organization_id, account_id) ON DELETE CASCADE,
    FOREIGN KEY (organization_id, policy_id) REFERENCES cloud_service_control_policies(organization_id, id) ON DELETE CASCADE
);

-- Policy management stays disabled while an older API replica may still
-- accept requests without enforcing SCPs. Enable after upgrading the fleet.
CREATE TABLE cloud_security_activation (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    scp_enabled boolean NOT NULL DEFAULT false
);
INSERT INTO cloud_security_activation (singleton) VALUES (true);

-- Backfill an independent management organization for each existing account.
INSERT INTO cloud_organizations (id, name, management_account_id)
    SELECT gen_random_uuid(), name, id FROM organizations;
INSERT INTO cloud_service_control_policies (id, organization_id, name, document, is_managed)
    SELECT gen_random_uuid(), id, 'FullAWSAccess',
        '{"version":"2026-07-31","statements":[{"effect":"Allow","actions":["*"],"resources":["*"]}]}'::jsonb,
        true FROM cloud_organizations;
INSERT INTO cloud_organizational_units (id, organization_id, parent_id, name)
    SELECT gen_random_uuid(), id, NULL, 'Root' FROM cloud_organizations;
INSERT INTO cloud_organization_accounts (account_id, organization_id, parent_id)
    SELECT o.management_account_id, o.id, u.id FROM cloud_organizations o
    JOIN cloud_organizational_units u ON u.organization_id = o.id AND u.parent_id IS NULL;
INSERT INTO cloud_scp_unit_attachments (organization_id, unit_id, policy_id)
    SELECT u.organization_id, u.id, p.id FROM cloud_organizational_units u
    JOIN cloud_service_control_policies p ON p.organization_id = u.organization_id AND p.is_managed;
INSERT INTO cloud_scp_account_attachments (organization_id, account_id, policy_id)
    SELECT a.organization_id, a.account_id, p.id FROM cloud_organization_accounts a
    JOIN cloud_service_control_policies p ON p.organization_id = a.organization_id AND p.is_managed;

CREATE FUNCTION attach_default_unit_scp() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO cloud_scp_unit_attachments (organization_id, unit_id, policy_id)
        SELECT NEW.organization_id, NEW.id, id FROM cloud_service_control_policies
        WHERE organization_id = NEW.organization_id AND is_managed;
    RETURN NEW;
END;
$$;
CREATE TRIGGER cloud_unit_attach_default_scp AFTER INSERT ON cloud_organizational_units
    FOR EACH ROW EXECUTE FUNCTION attach_default_unit_scp();

CREATE FUNCTION attach_default_account_scp() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO cloud_scp_account_attachments (organization_id, account_id, policy_id)
        SELECT NEW.organization_id, NEW.account_id, id FROM cloud_service_control_policies
        WHERE organization_id = NEW.organization_id AND is_managed;
    RETURN NEW;
END;
$$;
CREATE TRIGGER cloud_account_attach_default_scp AFTER INSERT ON cloud_organization_accounts
    FOR EACH ROW EXECUTE FUNCTION attach_default_account_scp();

-- All existing registration and invitation paths create accounts through the
-- old table. Keep them safe even when they do not know about the new model.
CREATE FUNCTION create_account_organization() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE new_organization_id uuid := gen_random_uuid();
DECLARE new_root_id uuid := gen_random_uuid();
BEGIN
    INSERT INTO cloud_organizations (id, name, management_account_id)
        VALUES (new_organization_id, NEW.name, NEW.id);
    INSERT INTO cloud_service_control_policies (id, organization_id, name, document, is_managed)
        VALUES (gen_random_uuid(), new_organization_id, 'FullAWSAccess',
            '{"version":"2026-07-31","statements":[{"effect":"Allow","actions":["*"],"resources":["*"]}]}'::jsonb, true);
    INSERT INTO cloud_organizational_units (id, organization_id, parent_id, name)
        VALUES (new_root_id, new_organization_id, NULL, 'Root');
    INSERT INTO cloud_organization_accounts (account_id, organization_id, parent_id)
        VALUES (NEW.id, new_organization_id, new_root_id);
    RETURN NEW;
END;
$$;
CREATE TRIGGER organizations_create_cloud_organization
    AFTER INSERT ON organizations FOR EACH ROW EXECUTE FUNCTION create_account_organization();
