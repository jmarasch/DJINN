-- What the thorough check found in each MP3: whether its bit rate varies
-- with no length header (so other players guess its length wrong). Kept for
-- the file's size and mtime; a changed file is checked again.
CREATE TABLE mp3_header_checks (
    track_id INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    size     INTEGER NOT NULL,
    mtime    INTEGER NOT NULL,
    missing  INTEGER NOT NULL
);
CREATE INDEX mp3_header_checks_missing ON mp3_header_checks (missing) WHERE missing = 1;
