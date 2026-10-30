-- Tag corrections made in the app. Files are never changed: each time a
-- file is read, these replace what its tags say. NULL keeps the file's
-- value.
CREATE TABLE tag_edits (
    track_id     INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    title        TEXT,
    artist       TEXT,
    album        TEXT,
    album_artist TEXT,
    year         INTEGER,
    genre        TEXT,
    edited_at    INTEGER NOT NULL
);
