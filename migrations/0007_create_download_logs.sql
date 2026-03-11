-- ── Download log ──────────────────────────────────────────────────────────────

CREATE TABLE download_logs (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    version_id UUID        NOT NULL REFERENCES package_versions(id) ON DELETE CASCADE,
    ip_hash    TEXT        NOT NULL,   -- SHA-256(ip) for privacy
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Time-series index for chart queries
CREATE INDEX download_logs_version_created_idx ON download_logs (version_id, created_at DESC);
CREATE INDEX download_logs_created_idx         ON download_logs (created_at DESC);
