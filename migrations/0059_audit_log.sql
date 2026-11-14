-- The audit trail of protected areas: who unlocked the editing screens, who
-- changed the lock's strictness, and what was changed inside. An admin can
-- turn it off and purge it (`audit_enabled` setting).
CREATE TABLE audit_log (
    id       INTEGER PRIMARY KEY,
    at       INTEGER NOT NULL,
    user_id  INTEGER,
    username TEXT    NOT NULL,
    ip       TEXT,
    -- entered, entry_failed, left, level_changed, audit_on, audit_off,
    -- audit_purged, approved_submission, ...
    kind     TEXT    NOT NULL,
    detail   TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX audit_log_at ON audit_log (at);
