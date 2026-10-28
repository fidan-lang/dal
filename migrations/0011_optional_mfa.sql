-- Durable downgrade guard only. Cognito owns every TOTP secret and verification.
CREATE TABLE user_mfa (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    required BOOLEAN NOT NULL DEFAULT false
);

-- Only hashes of completed MFA credentials; never raw JWTs, refresh tokens or TOTP secrets.
CREATE TABLE mfa_sessions (
    refresh_hash TEXT PRIMARY KEY CHECK (length(refresh_hash)=64),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + INTERVAL '30 days'
);
CREATE INDEX mfa_sessions_user ON mfa_sessions(user_id);
CREATE TABLE mfa_id_tokens (
    token_hash TEXT PRIMARY KEY CHECK (length(token_hash)=64),
    refresh_hash TEXT NOT NULL REFERENCES mfa_sessions(refresh_hash) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX mfa_id_tokens_session ON mfa_id_tokens(refresh_hash);
