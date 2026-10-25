-- Playlists replace the single auto pool. A playlist holds songs, albums,
-- and artists; albums and artists are expanded when played, so new music
-- by them joins automatically. One playlist at a time feeds the music when
-- nobody has requested anything (setting `active_playlist`).

ALTER TABLE playlists ADD COLUMN shuffle INTEGER NOT NULL DEFAULT 1;

CREATE TABLE playlist_entries (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN ('track', 'album', 'artist')),
    ref_id      INTEGER NOT NULL,
    position    INTEGER NOT NULL,
    added_at    INTEGER NOT NULL,
    UNIQUE (playlist_id, kind, ref_id)
);
CREATE INDEX playlist_entries_order ON playlist_entries (playlist_id, position);

-- Carry an existing auto pool over as a playlist (needs an admin to own it).
INSERT INTO playlists (owner_id, name, created_at, updated_at, shuffle)
SELECT (SELECT min(id) FROM users WHERE role = 'admin'), 'Party mix',
       CAST(strftime('%s', 'now') AS INTEGER) * 1000,
       CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1
WHERE EXISTS (SELECT 1 FROM pool_entries WHERE kind != 'playlist')
  AND EXISTS (SELECT 1 FROM users WHERE role = 'admin');

INSERT INTO playlist_entries (playlist_id, kind, ref_id, position, added_at)
SELECT (SELECT max(id) FROM playlists WHERE name = 'Party mix'), kind, ref_id,
       row_number() OVER (ORDER BY added_at, id), added_at
FROM pool_entries
WHERE kind != 'playlist' AND EXISTS (SELECT 1 FROM playlists WHERE name = 'Party mix');

INSERT INTO settings (key, value)
SELECT 'active_playlist', CAST(max(id) AS TEXT) FROM playlists WHERE name = 'Party mix'
HAVING max(id) IS NOT NULL;

DROP TABLE pool_entries;
-- Superseded by playlist_entries; never used.
DROP TABLE playlist_tracks;

-- Artist photos and bios looked up online (opt-in).
ALTER TABLE artists ADD COLUMN online_art_hash TEXT;
ALTER TABLE artists ADD COLUMN bio TEXT;
ALTER TABLE artists ADD COLUMN bio_url TEXT;
ALTER TABLE artists ADD COLUMN info_checked_at INTEGER;
