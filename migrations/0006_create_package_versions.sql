-- ── Package versions ──────────────────────────────────────────────────────────

CREATE TABLE package_versions (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    package_id   UUID        NOT NULL REFERENCES packages(id) ON DELETE CASCADE,
    version      TEXT        NOT NULL,
    checksum     TEXT        NOT NULL,   -- SHA-256 hex of .tar.gz
    size_bytes   BIGINT      NOT NULL,
    s3_key       TEXT        NOT NULL,
    readme       TEXT,
    manifest     JSONB       NOT NULL DEFAULT '{}',
    yanked       BOOLEAN     NOT NULL DEFAULT FALSE,
    yank_reason  TEXT,
    downloads    BIGINT      NOT NULL DEFAULT 0,
    published_by UUID        NOT NULL REFERENCES users(id),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX package_versions_pkg_ver_idx ON package_versions (package_id, version);
CREATE        INDEX package_versions_pkg_idx     ON package_versions (package_id);
CREATE        INDEX package_versions_created_idx ON package_versions (created_at DESC);
