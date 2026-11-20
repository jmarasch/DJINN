-- Accounts locked after too many wrong two-step codes (a lasting lock, kept
-- across restarts), and a note that a new password must be changed. An admin
-- can unlock another account in DJINN; `djinn recover` on the host is the
-- last resort when no admin can sign in.
CREATE TABLE account_security (
    user_id              INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    -- Wrong two-step codes in a row (after a right password).
    failures             INTEGER NOT NULL DEFAULT 0,
    locked_at            INTEGER,
    must_change_password INTEGER NOT NULL DEFAULT 0
);
