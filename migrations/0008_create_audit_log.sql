-- ── Audit log ─────────────────────────────────────────────────────────────────

CREATE TABLE audit_log (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    actor_id    UUID        REFERENCES users(id) ON DELETE SET NULL,
    action      TEXT        NOT NULL,           -- e.g. "publish", "yank", "add_owner"
    target_id   UUID,
    target_type TEXT,                           -- e.g. "version", "package", "user"
    metadata    JSONB       NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_log_actor_idx   ON audit_log (actor_id);
CREATE INDEX audit_log_target_idx  ON audit_log (target_id);
CREATE INDEX audit_log_created_idx ON audit_log (created_at DESC);
