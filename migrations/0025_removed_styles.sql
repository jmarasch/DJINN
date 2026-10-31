-- "Sounds like" genres taken off a song in the app. They stay off when the
-- AI listens to the song again; other songs keep the genre.
CREATE TABLE removed_styles (
    track_id INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    -- Normalized, as the AI's genres are.
    genre    TEXT    NOT NULL,
    PRIMARY KEY (track_id, genre)
);
