-- Roles: named sets of permissions for accounts. Admins and guests keep
-- their built-in kind in users.role; every other account has one of these.
-- Role 1, Listener, is built in: it can be edited but not deleted.
CREATE TABLE roles (
    id                    INTEGER PRIMARY KEY,
    name                  TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    builtin               INTEGER NOT NULL DEFAULT 0,
    -- DJINN GO: personal listening.
    go                    INTEGER NOT NULL DEFAULT 0,
    -- GO from outside the home network.
    go_remote             INTEGER NOT NULL DEFAULT 0,
    -- GO streams at once; 0 is no limit.
    max_streams           INTEGER NOT NULL DEFAULT 2,
    downloads             INTEGER NOT NULL DEFAULT 0,
    share_playlists       INTEGER NOT NULL DEFAULT 0,
    submit_music          INTEGER NOT NULL DEFAULT 0,
    submit_without_review INTEGER NOT NULL DEFAULT 0,
    -- Party requests skip the per-person limit and the queue cap.
    unlimited_requests    INTEGER NOT NULL DEFAULT 0,
    created_at            INTEGER NOT NULL
);

INSERT INTO roles (id, name, builtin, go, share_playlists, created_at)
VALUES (1, 'Listener', 1, 1, 1, CAST(strftime('%s', 'now') AS INTEGER) * 1000);

-- No REFERENCES: SQLite can't add one with a non-NULL default. Deleting a
-- role moves its accounts to Listener first.
ALTER TABLE users ADD COLUMN role_id INTEGER NOT NULL DEFAULT 1;
-- 'pending' accounts signed up and wait for the admin; 'disabled' ones
-- can't sign in.
ALTER TABLE users ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
    CHECK (status IN ('active', 'pending', 'disabled'));

-- Signed-in devices.
ALTER TABLE sessions ADD COLUMN last_seen_at INTEGER;
ALTER TABLE sessions ADD COLUMN ip TEXT;
ALTER TABLE sessions ADD COLUMN user_agent TEXT;

-- Invite links. Only the token's hash is kept; the link is shown once.
CREATE TABLE invites (
    id         INTEGER PRIMARY KEY,
    token_hash BLOB    NOT NULL UNIQUE,
    role_id    INTEGER NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    note       TEXT    NOT NULL DEFAULT '',
    -- NULL: no limit.
    uses_left  INTEGER,
    used       INTEGER NOT NULL DEFAULT 0,
    created_by INTEGER REFERENCES users (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    -- NULL: never.
    expires_at INTEGER
);
