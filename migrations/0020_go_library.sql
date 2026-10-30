-- DJINN GO: each person's own library. Nothing here feeds the party's
-- learning.

-- Hearts on songs, albums and artists.
CREATE TABLE go_likes (
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- 'track', 'album' or 'artist'.
    kind    TEXT    NOT NULL CHECK (kind IN ('track', 'album', 'artist')),
    ref_id  INTEGER NOT NULL,
    at      INTEGER NOT NULL,
    PRIMARY KEY (user_id, kind, ref_id)
);

-- Songs played in GO: history, and what personal radio learns from.
CREATE TABLE go_plays (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    track_id  INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    at        INTEGER NOT NULL,
    -- How much was heard, and whether it was skipped early.
    ms_played INTEGER NOT NULL DEFAULT 0,
    skipped   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX go_plays_user_at ON go_plays (user_id, at);

-- Where each person left off, so another device can carry on.
CREATE TABLE go_resume (
    user_id     INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    -- JSON array of track ids: the queue as it was.
    queue       TEXT    NOT NULL,
    position    INTEGER NOT NULL,
    position_ms INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- Each person's own playlists (the party's playlists are separate).
CREATE TABLE go_playlists (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    -- Other GO listeners can see and play it.
    shared     INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX go_playlists_user ON go_playlists (user_id);

CREATE TABLE go_playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES go_playlists (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    track_id    INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    PRIMARY KEY (playlist_id, position)
);
