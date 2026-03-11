-- ── Packages ──────────────────────────────────────────────────────────────────

CREATE TABLE packages (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT        NOT NULL,
    description TEXT,
    repository  TEXT,
    homepage    TEXT,
    license     TEXT,
    readme      TEXT,
    keywords    JSONB       NOT NULL DEFAULT '[]',
    categories  JSONB       NOT NULL DEFAULT '[]',
    downloads   BIGINT      NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX packages_name_lower_idx ON packages (lower(name));
CREATE        INDEX packages_downloads_idx  ON packages (downloads DESC);

-- Full-text search index on name + description
CREATE INDEX packages_fts_idx ON packages
    USING GIN (to_tsvector('english', name || ' ' || COALESCE(description, '')));
