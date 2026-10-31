-- Music people upload in DJINN GO. Files wait in <db dir>/uploads/ until
-- approved (then they move into the uploads library folder) or turned
-- down (then they're deleted).
CREATE TABLE submissions (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER REFERENCES users (id) ON DELETE SET NULL,
    -- The name it was uploaded with.
    file_name    TEXT    NOT NULL,
    -- Its name in the uploads folder while it waits.
    stored_name  TEXT    NOT NULL,
    size         INTEGER NOT NULL,
    sha256       BLOB    NOT NULL,
    -- From its tags.
    title        TEXT,
    artist       TEXT,
    album        TEXT,
    duration_ms  INTEGER,
    -- A song already in the library that looks like the same one.
    duplicate_of INTEGER REFERENCES tracks (id) ON DELETE SET NULL,
    -- 'waiting', 'approved' or 'declined'.
    status       TEXT    NOT NULL DEFAULT 'waiting'
        CHECK (status IN ('waiting', 'approved', 'declined')),
    -- Why it was turned down, for the uploader.
    note         TEXT,
    -- Where an approved file went.
    final_path   TEXT,
    created_at   INTEGER NOT NULL,
    decided_at   INTEGER,
    decided_by   INTEGER REFERENCES users (id) ON DELETE SET NULL
);
CREATE INDEX submissions_status ON submissions (status, created_at);
CREATE INDEX submissions_user ON submissions (user_id, created_at);
CREATE INDEX submissions_sha ON submissions (sha256);
