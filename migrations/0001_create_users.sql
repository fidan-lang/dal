-- Enable pgcrypto for gen_random_uuid()
CREATE EXTENSION IF NOT EXISTS pgcrypto;
-- Enable full-text search dictionary (already included in standard Postgres)
-- Enable uuid-ossp as fallback
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ── Users ─────────────────────────────────────────────────────────────────────

CREATE TABLE users (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    username       TEXT        NOT NULL,
    email          TEXT        NOT NULL,
    cognito_sub    TEXT        NOT NULL,
    display_name   TEXT,
    avatar_url     TEXT,
    bio            TEXT,
    website        TEXT,
    is_admin       BOOLEAN     NOT NULL DEFAULT FALSE,
    email_verified BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX users_username_lower_idx ON users (lower(username));
CREATE UNIQUE INDEX users_email_lower_idx    ON users (lower(email));
CREATE UNIQUE INDEX users_cognito_sub_idx    ON users (cognito_sub);
