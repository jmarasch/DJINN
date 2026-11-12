-- What people searched for (Admin > Stats): searches that found nothing
-- are a shopping list. Typing one search keeps one row.
CREATE TABLE search_log (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER REFERENCES users (id) ON DELETE SET NULL,
    query     TEXT    NOT NULL,
    results   INTEGER NOT NULL,
    at        INTEGER NOT NULL,
    -- They asked for a song soon after.
    requested INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX search_log_at ON search_log (at);
CREATE INDEX search_log_user ON search_log (user_id, at);

-- Songs people wish the library had ("Not in the library?"). One per
-- artist and title (`wanted`, folded); `status` is open, got or ignored.
CREATE TABLE wishes (
    id         INTEGER PRIMARY KEY,
    artist     TEXT    NOT NULL,
    title      TEXT    NOT NULL,
    wanted     TEXT    NOT NULL UNIQUE,
    status     TEXT    NOT NULL DEFAULT 'open',
    track_id   INTEGER REFERENCES tracks (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE wish_askers (
    wish_id INTEGER NOT NULL REFERENCES wishes (id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    at      INTEGER NOT NULL,
    PRIMARY KEY (wish_id, user_id)
);
