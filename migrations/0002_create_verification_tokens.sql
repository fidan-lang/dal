-- ── Verification tokens (email verify + password reset) ───────────────────────

CREATE TABLE verification_tokens (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id     UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT        NOT NULL,
    kind        TEXT        NOT NULL CHECK (kind IN ('email_verify', 'password_reset')),
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One active token per (user, kind) — upserted on re-request
CREATE UNIQUE INDEX verification_tokens_user_kind_idx ON verification_tokens (user_id, kind);
CREATE        INDEX verification_tokens_hash_idx      ON verification_tokens (token_hash);
