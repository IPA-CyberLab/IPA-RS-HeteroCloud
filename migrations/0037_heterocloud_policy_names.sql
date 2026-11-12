-- Rename only the platform-managed default. Preserve policy IDs, documents and
-- every attachment; user-created policies with the same desired name win.
DO $$
DECLARE
    policy_row record;
    new_name text;
BEGIN
    FOR policy_row IN
        SELECT id, organization_id FROM cloud_service_control_policies
        WHERE is_managed AND name = 'FullAWSAccess' FOR UPDATE
    LOOP
        new_name := 'FullHeteroCloudAccess';
        IF EXISTS (SELECT 1 FROM cloud_service_control_policies
                   WHERE organization_id = policy_row.organization_id AND name = new_name) THEN
            new_name := 'FullHeteroCloudAccess-' || replace(policy_row.id::text, '-', '');
        END IF;
        WHILE EXISTS (SELECT 1 FROM cloud_service_control_policies
                      WHERE organization_id = policy_row.organization_id AND name = new_name) LOOP
            new_name := 'FullHeteroCloudAccess-' || replace(gen_random_uuid()::text, '-', '');
        END LOOP;
        UPDATE cloud_service_control_policies SET name = new_name WHERE id = policy_row.id;
    END LOOP;
END;
$$;

-- The existing trigger also handles organizations created by older clients.
CREATE OR REPLACE FUNCTION create_account_organization() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE new_organization_id uuid := gen_random_uuid();
DECLARE new_root_id uuid := gen_random_uuid();
BEGIN
    INSERT INTO cloud_organizations (id, name, management_account_id)
        VALUES (new_organization_id, NEW.name, NEW.id);
    INSERT INTO cloud_service_control_policies (id, organization_id, name, document, is_managed)
        VALUES (gen_random_uuid(), new_organization_id, 'FullHeteroCloudAccess',
            '{"version":"2026-07-31","statements":[{"effect":"Allow","actions":["*"],"resources":["*"]}]}'::jsonb, true);
    INSERT INTO cloud_organizational_units (id, organization_id, parent_id, name)
        VALUES (new_root_id, new_organization_id, NULL, 'Root');
    INSERT INTO cloud_organization_accounts (account_id, organization_id, parent_id)
        VALUES (NEW.id, new_organization_id, new_root_id);
    RETURN NEW;
END;
$$;
