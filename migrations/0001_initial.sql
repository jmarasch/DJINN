-- Initial schema. Timestamps are Unix epoch milliseconds (INTEGER).

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT    NOT NULL,
    -- 'guest' is reserved for party access (Phase 4).
    role          TEXT    NOT NULL CHECK (role IN ('admin', 'user', 'guest')),
    created_at    INTEGER NOT NULL
);

CREATE TABLE sessions (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash BLOB    NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions (user_id);

-- Artists and albums are derived from track tags by the scanner so they have
-- stable ids for the API and the auto pool.
CREATE TABLE artists (
    id   INTEGER PRIMARY KEY,
    name TEXT    NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE albums (
    id        INTEGER PRIMARY KEY,
    title     TEXT    NOT NULL COLLATE NOCASE,
    -- Album artist, falling back to the track artist when the tag is missing.
    artist_id INTEGER NOT NULL REFERENCES artists (id),
    year      INTEGER,
    art_hash  TEXT,
    UNIQUE (title, artist_id)
);
CREATE INDEX albums_artist ON albums (artist_id);

CREATE TABLE tracks (
    id           INTEGER PRIMARY KEY,
    path         TEXT    NOT NULL UNIQUE,
    size         INTEGER NOT NULL,
    mtime        INTEGER NOT NULL,
    duration_ms  INTEGER,
    title        TEXT    NOT NULL,
    artist       TEXT,
    album        TEXT,
    album_artist TEXT,
    track_no     INTEGER,
    disc_no      INTEGER,
    year         INTEGER,
    genre        TEXT,
    art_hash     TEXT,
    sample_rate  INTEGER,
    channels     INTEGER,
    codec        TEXT,
    artist_id    INTEGER REFERENCES artists (id),
    album_id     INTEGER REFERENCES albums (id),
    added_at     INTEGER NOT NULL,
    -- Set when the file disappears from disk. Rows are kept so play history
    -- stays intact; a reappearing file at the same path clears it.
    removed_at   INTEGER
);
CREATE INDEX tracks_artist ON tracks (artist_id);
CREATE INDEX tracks_album ON tracks (album_id, disc_no, track_no);

-- Full-text search. Standalone FTS5 tables kept in sync by triggers; rowid
-- matches the source table's id.
CREATE VIRTUAL TABLE tracks_fts USING fts5 (
    title, artist, album, album_artist, genre,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER tracks_fts_insert AFTER INSERT ON tracks BEGIN
    INSERT INTO tracks_fts (rowid, title, artist, album, album_artist, genre)
    VALUES (new.id, new.title, new.artist, new.album, new.album_artist, new.genre);
END;

CREATE TRIGGER tracks_fts_update AFTER UPDATE OF title, artist, album, album_artist, genre ON tracks BEGIN
    DELETE FROM tracks_fts WHERE rowid = old.id;
    INSERT INTO tracks_fts (rowid, title, artist, album, album_artist, genre)
    VALUES (new.id, new.title, new.artist, new.album, new.album_artist, new.genre);
END;

CREATE TRIGGER tracks_fts_delete AFTER DELETE ON tracks BEGIN
    DELETE FROM tracks_fts WHERE rowid = old.id;
END;

CREATE VIRTUAL TABLE artists_fts USING fts5 (
    name,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER artists_fts_insert AFTER INSERT ON artists BEGIN
    INSERT INTO artists_fts (rowid, name) VALUES (new.id, new.name);
END;

CREATE TRIGGER artists_fts_delete AFTER DELETE ON artists BEGIN
    DELETE FROM artists_fts WHERE rowid = old.id;
END;

CREATE VIRTUAL TABLE albums_fts USING fts5 (
    title, artist,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER albums_fts_insert AFTER INSERT ON albums BEGIN
    INSERT INTO albums_fts (rowid, title, artist)
    VALUES (new.id, new.title, (SELECT name FROM artists WHERE id = new.artist_id));
END;

CREATE TRIGGER albums_fts_delete AFTER DELETE ON albums BEGIN
    DELETE FROM albums_fts WHERE rowid = old.id;
END;

CREATE TABLE playlists (
    id         INTEGER PRIMARY KEY,
    owner_id   INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX playlists_owner ON playlists (owner_id);

CREATE TABLE playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    track_id    INTEGER NOT NULL REFERENCES tracks (id),
    position    INTEGER NOT NULL,
    PRIMARY KEY (playlist_id, position)
);

-- The approved auto pool. Album, artist, and playlist entries are expanded
-- when a track is drawn, so tracks added to them later join the pool.
CREATE TABLE pool_entries (
    id       INTEGER PRIMARY KEY,
    kind     TEXT    NOT NULL CHECK (kind IN ('track', 'album', 'artist', 'playlist')),
    ref_id   INTEGER NOT NULL,
    added_at INTEGER NOT NULL,
    UNIQUE (kind, ref_id)
);

-- Ordered by position; added_at is informational.
CREATE TABLE request_queue (
    id       INTEGER PRIMARY KEY,
    track_id INTEGER NOT NULL UNIQUE REFERENCES tracks (id),
    user_id  INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL,
    position INTEGER NOT NULL
);
CREATE INDEX request_queue_position ON request_queue (position);
CREATE INDEX request_queue_user ON request_queue (user_id);

CREATE TABLE outputs (
    id          INTEGER PRIMARY KEY,
    device_name TEXT    NOT NULL UNIQUE,
    enabled     INTEGER NOT NULL DEFAULT 1,
    volume_trim REAL    NOT NULL DEFAULT 1.0,
    delay_ms    INTEGER NOT NULL DEFAULT 0 CHECK (delay_ms BETWEEN 0 AND 1000)
);

-- Runtime-tunable settings. Missing keys fall back to defaults in code.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Listening history for the future smart playlist feature.
CREATE TABLE play_events (
    id           INTEGER PRIMARY KEY,
    track_id     INTEGER NOT NULL REFERENCES tracks (id),
    source       TEXT    NOT NULL CHECK (source IN ('request', 'pool')),
    requested_by INTEGER REFERENCES users (id) ON DELETE SET NULL,
    started_at   INTEGER NOT NULL,
    ended_at     INTEGER,
    played_ms    INTEGER,
    skipped      INTEGER NOT NULL DEFAULT 0,
    skipped_by   INTEGER REFERENCES users (id) ON DELETE SET NULL
);
CREATE INDEX play_events_started ON play_events (started_at);
CREATE INDEX play_events_track ON play_events (track_id);

CREATE TABLE request_events (
    id       INTEGER PRIMARY KEY,
    track_id INTEGER NOT NULL REFERENCES tracks (id),
    user_id  INTEGER REFERENCES users (id) ON DELETE SET NULL,
    at       INTEGER NOT NULL
);
CREATE INDEX request_events_track ON request_events (track_id);
