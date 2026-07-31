UPDATE invitations
SET revoked_at = COALESCE(revoked_at, now())
WHERE max_uses <> 1 OR used_count > 1;

UPDATE invitations
SET max_uses = 1,
    used_count = LEAST(used_count, 1),
    expires_at = GREATEST(
        created_at + interval '1 second',
        LEAST(expires_at, created_at + interval '24 hours')
    );

ALTER TABLE invitations
    ALTER COLUMN max_uses SET DEFAULT 1,
    DROP CONSTRAINT invitations_max_uses_check,
    ADD CONSTRAINT invitations_single_use_check CHECK (max_uses = 1),
    ADD CONSTRAINT invitations_short_ttl_check CHECK (
        expires_at > created_at
        AND expires_at <= created_at + interval '24 hours'
    );
