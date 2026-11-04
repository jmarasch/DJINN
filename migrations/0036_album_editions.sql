-- Albums with one name can be different releases, told apart by their folders.
-- `edition` is what keeps them apart: empty for the biggest part, the folder's
-- name for the others. `label` is the folder's name to show, on every part of
-- an album that was split. SQLite can't change a UNIQUE in place, so the table
-- is made again; the ids stay, and the songs point at them all along.
PRAGMA defer_foreign_keys = ON;

-- The rows have to go back into a table named albums for the songs' references
-- to count as met, so they wait in a copy while it's made again.
CREATE TABLE albums_copy AS
    SELECT id, title, artist_id, year, art_hash, mbid, genres, online_art_hash, info_checked_at FROM albums;
DROP TABLE albums;

CREATE TABLE albums (
    id              INTEGER PRIMARY KEY,
    title           TEXT    NOT NULL COLLATE NOCASE,
    -- Album artist, falling back to the track artist when the tag is missing.
    artist_id       INTEGER NOT NULL REFERENCES artists (id),
    year            INTEGER,
    art_hash        TEXT,
    mbid            TEXT,
    genres          TEXT,
    online_art_hash TEXT,
    info_checked_at INTEGER,
    edition         TEXT    NOT NULL DEFAULT '',
    label           TEXT,
    UNIQUE (title, artist_id, edition)
);
INSERT INTO albums (id, title, artist_id, year, art_hash, mbid, genres, online_art_hash, info_checked_at)
    SELECT id, title, artist_id, year, art_hash, mbid, genres, online_art_hash, info_checked_at FROM albums_copy;
DROP TABLE albums_copy;

CREATE INDEX albums_artist ON albums (artist_id);
CREATE TRIGGER albums_fts_insert AFTER INSERT ON albums BEGIN
    INSERT INTO albums_fts (rowid, title, artist)
    VALUES (new.id, new.title, (SELECT name FROM artists WHERE id = new.artist_id));
END;
CREATE TRIGGER albums_fts_delete AFTER DELETE ON albums BEGIN
    DELETE FROM albums_fts WHERE rowid = old.id;
END;
CREATE TRIGGER browse_albums_insert AFTER INSERT ON albums
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_albums_delete AFTER DELETE ON albums
BEGIN UPDATE browse_version SET version = version + 1; END;
CREATE TRIGGER browse_albums_update AFTER UPDATE OF
    title, artist_id, year, art_hash, genres, label ON albums
BEGIN UPDATE browse_version SET version = version + 1; END;
