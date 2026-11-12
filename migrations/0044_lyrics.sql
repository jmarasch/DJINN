-- Song lyrics, kept apart from tracks so library checks stay cheap.
-- `synced` is LRC text (timed lines), `plain` the words alone (always set
-- when there are words, since search reads it). `source` is where they came
-- from: 'lrc' (a .lrc file beside the song), 'file' (its tags), 'lrclib'
-- (looked up online), or 'none' (nothing found yet).
CREATE TABLE lyrics (
    track_id          INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    synced            TEXT,
    plain             TEXT,
    instrumental      INTEGER NOT NULL DEFAULT 0,
    source            TEXT    NOT NULL CHECK (source IN ('lrc', 'file', 'lrclib', 'none')),
    -- The song file's mtime when it was read, so a changed file is read again.
    file_mtime        INTEGER,
    -- When LRCLIB was last asked about it.
    online_checked_at INTEGER,
    updated_at        INTEGER NOT NULL
);
CREATE INDEX lyrics_source ON lyrics (source);

CREATE VIRTUAL TABLE lyrics_fts USING fts5 (
    plain,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER lyrics_fts_insert AFTER INSERT ON lyrics WHEN new.plain IS NOT NULL BEGIN
    INSERT INTO lyrics_fts (rowid, plain) VALUES (new.track_id, new.plain);
END;

CREATE TRIGGER lyrics_fts_update AFTER UPDATE OF plain ON lyrics BEGIN
    DELETE FROM lyrics_fts WHERE rowid = old.track_id;
    INSERT INTO lyrics_fts (rowid, plain) SELECT new.track_id, new.plain WHERE new.plain IS NOT NULL;
END;

CREATE TRIGGER lyrics_fts_delete AFTER DELETE ON lyrics BEGIN
    DELETE FROM lyrics_fts WHERE rowid = old.track_id;
END;
