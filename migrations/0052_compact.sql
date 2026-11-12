-- A smaller database.
--
-- What the AI heard (2 KB a song) moves to its own table. Beside the
-- fingerprint in `track_sound`, each row was just too big for two to share
-- a page, so about a quarter of every page stood empty. `Store::open`
-- then stores it at half precision (1 KB a song), once, since SQL can't.
CREATE TABLE track_heard (
    track_id INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    -- Little-endian f16s; NULL when the model heard nothing.
    heard    BLOB,
    version  INTEGER NOT NULL
);

INSERT INTO track_heard (track_id, heard, version)
SELECT track_id, heard, heard_version FROM track_sound WHERE heard_version IS NOT NULL;

DROP TRIGGER tracks_file_changed;
ALTER TABLE track_sound DROP COLUMN heard;
ALTER TABLE track_sound DROP COLUMN heard_version;

-- A changed file gets analyzed and listened to again.
CREATE TRIGGER tracks_file_changed AFTER UPDATE OF size, mtime ON tracks
WHEN old.size <> new.size OR old.mtime <> new.mtime
BEGIN
    UPDATE track_sound SET features_version = NULL WHERE track_id = new.id;
    DELETE FROM track_heard WHERE track_id = new.id;
END;

INSERT INTO settings (key, value)
SELECT 'heard_to_half', '1' WHERE EXISTS (SELECT 1 FROM track_heard)
ON CONFLICT (key) DO UPDATE SET value = excluded.value;

-- Lyrics search reads the words from `lyrics` instead of keeping its own
-- copy of every song's lyrics.
DROP TRIGGER lyrics_fts_insert;
DROP TRIGGER lyrics_fts_update;
DROP TRIGGER lyrics_fts_delete;
DROP TABLE lyrics_fts;

CREATE VIRTUAL TABLE lyrics_fts USING fts5 (
    plain,
    content = 'lyrics',
    content_rowid = 'track_id',
    tokenize = 'unicode61 remove_diacritics 2'
);
INSERT INTO lyrics_fts (lyrics_fts) VALUES ('rebuild');

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

-- Lyrics are looked up online on new installs. Existing ones keep their
-- choice: off unless the admin turned it on.
INSERT INTO settings (key, value)
SELECT 'lyrics_lookup', '0' WHERE EXISTS (SELECT 1 FROM tracks)
ON CONFLICT (key) DO NOTHING;

-- Dropped columns and the old copy leave their pages behind until a VACUUM.
INSERT INTO settings (key, value) VALUES ('compact_database', '1')
ON CONFLICT (key) DO UPDATE SET value = excluded.value;
