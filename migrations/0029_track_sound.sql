-- How songs sound, moved out of `tracks`. At about 2 KB a song this filled
-- nearly all of the table, so anything reading the song list had to read
-- through it (147 MB instead of 5 MB on 36,000 songs).
CREATE TABLE track_sound (
    track_id         INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    features         BLOB,
    features_version INTEGER,
    bpm              REAL,
    heard            BLOB,
    heard_version    INTEGER
);

INSERT INTO track_sound (track_id, features, features_version, bpm, heard, heard_version)
SELECT id, features, features_version, bpm, heard, heard_version FROM tracks
WHERE features IS NOT NULL OR features_version IS NOT NULL OR bpm IS NOT NULL
   OR heard IS NOT NULL OR heard_version IS NOT NULL;

ALTER TABLE tracks DROP COLUMN features;
ALTER TABLE tracks DROP COLUMN features_version;
ALTER TABLE tracks DROP COLUMN bpm;
ALTER TABLE tracks DROP COLUMN heard;
ALTER TABLE tracks DROP COLUMN heard_version;

-- A changed file gets analyzed and listened to again.
CREATE TRIGGER tracks_file_changed AFTER UPDATE OF size, mtime ON tracks
WHEN old.size <> new.size OR old.mtime <> new.mtime
BEGIN
    UPDATE track_sound SET features_version = NULL, heard_version = NULL
    WHERE track_id = new.id;
END;

-- Dropping columns leaves the table's pages where they were; the space only
-- comes back with a VACUUM, which can't run in a migration. `Store::open`
-- does it once.
INSERT INTO settings (key, value) VALUES ('compact_database', '1')
ON CONFLICT (key) DO UPDATE SET value = excluded.value;

-- Goes up whenever a library list could change, so the lists kept in memory
-- (`djinn-store::library::browse`) are never out of date, whatever made the
-- change.
CREATE TABLE browse_version (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL
);
INSERT INTO browse_version (id, version) VALUES (1, 0);

CREATE TRIGGER browse_tracks_insert AFTER INSERT ON tracks
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_tracks_delete AFTER DELETE ON tracks
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_tracks_update AFTER UPDATE OF
    title, artist, album, album_artist, artist_id, album_id, track_no, disc_no,
    year, genre, added_at, removed_at ON tracks
BEGIN UPDATE browse_version SET version = version + 1; END;

CREATE TRIGGER browse_albums_insert AFTER INSERT ON albums
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_albums_delete AFTER DELETE ON albums
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_albums_update AFTER UPDATE OF
    title, artist_id, year, art_hash, genres ON albums
BEGIN UPDATE browse_version SET version = version + 1; END;

CREATE TRIGGER browse_artists_insert AFTER INSERT ON artists
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_artists_delete AFTER DELETE ON artists
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_artists_update AFTER UPDATE OF name, art_hash ON artists
BEGIN UPDATE browse_version SET version = version + 1; END;

CREATE TRIGGER browse_credits_insert AFTER INSERT ON track_artists
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_credits_delete AFTER DELETE ON track_artists
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_credits_update AFTER UPDATE ON track_artists
BEGIN UPDATE browse_version SET version = version + 1; END;
