-- App passwords for Subsonic phone apps on DJINN GO. Subsonic's sign-in
-- (a salted MD5 token) needs the server to know the password itself, so
-- these are random passwords made by DJINN, kept as they are, used only by
-- the Subsonic API, and removable any time. Account passwords stay hashed
-- and are never accepted there.
CREATE TABLE subsonic_keys (
    user_id      INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    secret       TEXT    NOT NULL UNIQUE,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER
);
