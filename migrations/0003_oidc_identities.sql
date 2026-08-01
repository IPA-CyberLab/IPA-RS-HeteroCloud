ALTER TABLE users
    ALTER COLUMN password_hash DROP NOT NULL;

CREATE TABLE user_external_identities (
    issuer text NOT NULL CHECK (char_length(issuer) BETWEEN 1 AND 2048),
    subject text NOT NULL CHECK (char_length(subject) BETWEEN 1 AND 255),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (issuer, subject),
    UNIQUE (user_id, issuer)
);

CREATE INDEX user_external_identities_user_id_idx
    ON user_external_identities (user_id);
