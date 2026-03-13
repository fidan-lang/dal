-- ── API token scopes ─────────────────────────────────────────────────────────

ALTER TABLE api_tokens
    ADD COLUMN scopes TEXT[] NOT NULL DEFAULT ARRAY['publish:new', 'publish:update', 'yank']::TEXT[];

-- ── Ownership invites ────────────────────────────────────────────────────────

CREATE TABLE owner_invites (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    package_id  UUID        NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
    invitee_id  UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    inviter_id  UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT        NOT NULL,
    role        TEXT        NOT NULL CHECK (role IN ('owner', 'collaborator')),
    accepted_at TIMESTAMPTZ,
    declined_at TIMESTAMPTZ,
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (package_id, invitee_id)
);

CREATE UNIQUE INDEX owner_invites_token_hash_idx ON owner_invites (token_hash);
CREATE INDEX owner_invites_invitee_idx ON owner_invites (invitee_id);
CREATE INDEX owner_invites_package_idx ON owner_invites (package_id);
