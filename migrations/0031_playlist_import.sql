-- Songs an imported playlist names that the library doesn't have. Their
-- place in the list is a playlist entry of kind 'wanted'; a library check
-- swaps it for the real song when one turns up.
CREATE TABLE playlist_wanted (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    title       TEXT    NOT NULL,
    artist      TEXT,
    album       TEXT,
    duration_ms INTEGER,
    -- The file the source named, for playlists from other music players.
    path        TEXT,
    -- "A - B" text that could be artist then title or the other way round.
    either_way  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX playlist_wanted_list ON playlist_wanted (playlist_id);

-- SQLite can't change a CHECK, so the table is rebuilt to allow 'wanted'.
CREATE TABLE playlist_entries_new (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN ('track', 'album', 'artist', 'wanted')),
    ref_id      INTEGER NOT NULL,
    position    INTEGER NOT NULL,
    added_at    INTEGER NOT NULL,
    UNIQUE (playlist_id, kind, ref_id)
);
INSERT INTO playlist_entries_new (id, playlist_id, kind, ref_id, position, added_at)
SELECT id, playlist_id, kind, ref_id, position, added_at FROM playlist_entries;
DROP TABLE playlist_entries;
ALTER TABLE playlist_entries_new RENAME TO playlist_entries;
CREATE INDEX playlist_entries_order ON playlist_entries (playlist_id, position);

-- Files that exist but won't play. Kept apart from tracks so that browse
-- triggers and scans don't notice. The file's size and mtime tell a later
-- scan whether it has been replaced since.
CREATE TABLE track_problems (
    track_id   INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    error      TEXT    NOT NULL,
    size       INTEGER,
    mtime      INTEGER,
    noticed_at INTEGER NOT NULL
);
