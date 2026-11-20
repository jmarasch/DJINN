-- Lyrics an admin removed as wrong or bad, and where they came from. A song's
-- lyrics from that source are refused while their text is the same, so a
-- bad lyric isn't fetched, imported or read again until it's fixed (the
-- source gives different text) or the admin allows it again.
CREATE TABLE lyrics_blocks (
    track_id   INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    source     TEXT    NOT NULL CHECK (source IN ('lrc', 'file', 'lrclib', 'bulk')),
    -- Which text was refused (a fingerprint, not the text itself).
    text_hash  TEXT    NOT NULL,
    reason     TEXT,
    blocked_by INTEGER REFERENCES users (id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (track_id, source)
);
