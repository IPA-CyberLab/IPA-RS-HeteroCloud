CREATE TABLE user_login_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_id uuid REFERENCES sessions(id) ON DELETE SET NULL,
    source_ip inet,
    authentication_method text NOT NULL CHECK (authentication_method IN ('local', 'oidc')),
    occurred_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX user_login_events_user_time_idx
    ON user_login_events (user_id, occurred_at DESC, id DESC);
