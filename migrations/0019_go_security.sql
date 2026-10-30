-- Sign-in history, for Admin > GO. Kept for 90 days.
CREATE TABLE sign_ins (
    id         INTEGER PRIMARY KEY,
    at         INTEGER NOT NULL,
    user_id    INTEGER REFERENCES users (id) ON DELETE SET NULL,
    -- As typed, so failed tries for unknown names show too.
    username   TEXT    NOT NULL,
    ip         TEXT,
    user_agent TEXT,
    -- 'party' (the main address) or 'go'.
    via        TEXT    NOT NULL,
    -- 'ok', or why not: 'wrong_password', 'locked_out', 'waiting',
    -- 'turned_off', 'wrong_code', 'no_go', 'not_home'.
    outcome    TEXT    NOT NULL
);
CREATE INDEX sign_ins_at ON sign_ins (at);

-- Two-step sign-in (TOTP). The secret is base32; `totp_pending` holds a
-- new one until the user confirms a code from it. `totp_last_step` stops a
-- code being used twice.
ALTER TABLE users ADD COLUMN totp_secret TEXT;
ALTER TABLE users ADD COLUMN totp_pending TEXT;
ALTER TABLE users ADD COLUMN totp_last_step INTEGER;

-- One-time codes for when the phone with the authenticator is lost. Only
-- hashes are kept.
CREATE TABLE recovery_codes (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash BLOB    NOT NULL,
    used_at   INTEGER
);
CREATE INDEX recovery_codes_user ON recovery_codes (user_id);

-- Accounts in this role must use two-step sign-in for DJINN GO.
ALTER TABLE roles ADD COLUMN require_two_step INTEGER NOT NULL DEFAULT 0;
