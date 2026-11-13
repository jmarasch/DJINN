-- Lyrics can come from an imported zip: source 'bulk'. SQLite can't change a
-- CHECK, so the table is rebuilt with the new value allowed.
DROP TRIGGER lyrics_fts_insert;
DROP TRIGGER lyrics_fts_update;
DROP TRIGGER lyrics_fts_delete;
DROP INDEX lyrics_source;
DROP INDEX lyrics_words;
DROP INDEX lyrics_timed;

ALTER TABLE lyrics RENAME TO lyrics_before_bulk;

CREATE TABLE lyrics (
    track_id          INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    synced            TEXT,
    plain             TEXT,
    instrumental      INTEGER NOT NULL DEFAULT 0,
    source            TEXT    NOT NULL CHECK (source IN ('lrc', 'file', 'lrclib', 'bulk', 'none')),
    -- The song file's mtime when it was read, so a changed file is read again.
    file_mtime        INTEGER,
    -- When LRCLIB was last asked about it.
    online_checked_at INTEGER,
    updated_at        INTEGER NOT NULL
);

INSERT INTO lyrics (track_id, synced, plain, instrumental, source, file_mtime, online_checked_at, updated_at)
SELECT track_id, synced, plain, instrumental, source, file_mtime, online_checked_at, updated_at
FROM lyrics_before_bulk;

DROP TABLE lyrics_before_bulk;

CREATE INDEX lyrics_source ON lyrics (source);
CREATE INDEX lyrics_words ON lyrics (track_id) WHERE synced IS NOT NULL OR plain IS NOT NULL;
CREATE INDEX lyrics_timed ON lyrics (track_id) WHERE synced IS NOT NULL;

CREATE TRIGGER lyrics_fts_insert AFTER INSERT ON lyrics WHEN new.plain IS NOT NULL BEGIN
    INSERT INTO lyrics_fts (rowid, plain) VALUES (new.track_id, new.plain);
END;

CREATE TRIGGER lyrics_fts_update AFTER UPDATE OF plain ON lyrics BEGIN
    INSERT INTO lyrics_fts (lyrics_fts, rowid, plain)
    SELECT 'delete', old.track_id, old.plain WHERE old.plain IS NOT NULL;
    INSERT INTO lyrics_fts (rowid, plain)
    SELECT new.track_id, new.plain WHERE new.plain IS NOT NULL;
END;

CREATE TRIGGER lyrics_fts_delete AFTER DELETE ON lyrics WHEN old.plain IS NOT NULL BEGIN
    INSERT INTO lyrics_fts (lyrics_fts, rowid, plain) VALUES ('delete', old.track_id, old.plain);
END;

INSERT INTO lyrics_fts (lyrics_fts) VALUES ('rebuild');
