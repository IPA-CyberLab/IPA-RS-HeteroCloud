-- A browser session may switch to one assumed role. Retain the reference when
-- the role expires or is revoked so that authorization fails closed instead
-- of silently falling back to the original user's more powerful identity.
ALTER TABLE sessions ADD COLUMN active_role_session_id uuid REFERENCES iam_role_sessions(id) ON DELETE RESTRICT;
CREATE UNIQUE INDEX sessions_active_role_session_key ON sessions (active_role_session_id)
    WHERE active_role_session_id IS NOT NULL;
ALTER TABLE cloud_security_activation ADD COLUMN browser_roles_enabled boolean NOT NULL DEFAULT false;
