-- ── API tokens (CLI publish credentials) ─────────────────────────────────────

CREATE TABLE api_tokens (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id      UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT        NOT NULL,
    token_hash   TEXT        NOT NULL,   -- SHA-256(raw_token)
    prefix       TEXT        NOT NULL,   -- first 8 chars (display only)
    last_used_at TIMESTAMPTZ,
    expires_at   TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX api_tokens_hash_idx ON api_tokens (token_hash);
CREATE        INDEX api_tokens_user_idx ON api_tokens (user_id);
