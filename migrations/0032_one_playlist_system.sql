-- Party playlists and GO playlists become one thing: a person's playlists,
-- which they can use everywhere. Each is private to its owner unless shared
-- with everyone (`shared`) or with particular people (`playlist_shares`).

-- May start a playlist as the auto mix. Admins always may.
ALTER TABLE roles ADD COLUMN auto_mix INTEGER NOT NULL DEFAULT 0;

CREATE TABLE playlists_new (
    id         INTEGER PRIMARY KEY,
    owner_id   INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    shuffle    INTEGER NOT NULL DEFAULT 1,
    shared     INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- GO playlists keep their ids: phones and Subsonic apps have them saved.
INSERT INTO playlists_new (id, owner_id, name, shuffle, shared, created_at, updated_at)
SELECT id, user_id, name, 1, shared, created_at, updated_at FROM go_playlists;

-- Party playlists go above them. They were open to every admin, so they stay
-- usable by shared with everyone.
INSERT INTO playlists_new (id, owner_id, name, shuffle, shared, created_at, updated_at)
SELECT id + (SELECT coalesce(max(id), 0) FROM go_playlists),
       owner_id, name, shuffle, 1, created_at, updated_at
FROM playlists;

UPDATE settings
SET value = CAST(CAST(value AS INTEGER) + (SELECT coalesce(max(id), 0) FROM go_playlists) AS TEXT)
WHERE key = 'active_playlist' AND CAST(value AS INTEGER) > 0;

CREATE TABLE playlist_entries_new (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlists_new (id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN ('track', 'album', 'artist', 'wanted')),
    ref_id      INTEGER NOT NULL,
    position    INTEGER NOT NULL,
    added_at    INTEGER NOT NULL,
    UNIQUE (playlist_id, kind, ref_id)
);
INSERT INTO playlist_entries_new (id, playlist_id, kind, ref_id, position, added_at)
SELECT id, playlist_id + (SELECT coalesce(max(id), 0) FROM go_playlists),
       kind, ref_id, position, added_at
FROM playlist_entries;

-- A song in a GO playlist more than once keeps its first place.
INSERT OR IGNORE INTO playlist_entries_new (playlist_id, kind, ref_id, position, added_at)
SELECT t.playlist_id, 'track', t.track_id, t.position + 1, p.updated_at
FROM go_playlist_tracks t JOIN go_playlists p ON p.id = t.playlist_id
ORDER BY t.playlist_id, t.position;

CREATE TABLE playlist_wanted_new (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlists_new (id) ON DELETE CASCADE,
    title       TEXT    NOT NULL,
    artist      TEXT,
    album       TEXT,
    duration_ms INTEGER,
    path        TEXT,
    either_way  INTEGER NOT NULL DEFAULT 0
);
INSERT INTO playlist_wanted_new (id, playlist_id, title, artist, album, duration_ms, path, either_way)
SELECT id, playlist_id + (SELECT coalesce(max(id), 0) FROM go_playlists),
       title, artist, album, duration_ms, path, either_way
FROM playlist_wanted;

DROP TABLE go_playlist_tracks;
DROP TABLE go_playlists;
DROP TABLE playlist_wanted;
DROP TABLE playlist_entries;
DROP TABLE playlists;

ALTER TABLE playlists_new RENAME TO playlists;
ALTER TABLE playlist_entries_new RENAME TO playlist_entries;
ALTER TABLE playlist_wanted_new RENAME TO playlist_wanted;

CREATE INDEX playlists_owner ON playlists (owner_id);
CREATE INDEX playlist_entries_order ON playlist_entries (playlist_id, position);
CREATE INDEX playlist_wanted_list ON playlist_wanted (playlist_id);

CREATE TABLE playlist_shares (
    playlist_id INTEGER NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    user_id     INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (playlist_id, user_id)
);
CREATE INDEX playlist_shares_user ON playlist_shares (user_id);
