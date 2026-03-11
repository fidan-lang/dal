-- ── Package owners ────────────────────────────────────────────────────────────

CREATE TABLE package_owners (
    package_id UUID        NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
    user_id    UUID        NOT NULL REFERENCES users(id)    ON DELETE CASCADE,
    role       TEXT        NOT NULL DEFAULT 'owner' CHECK (role IN ('owner', 'collaborator')),
    invited_by UUID        REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (package_id, user_id)
);

CREATE INDEX package_owners_user_idx ON package_owners (user_id);
