-- Listeners flagging a song that needs fixing (wrong info, wrong cover,
-- sounds bad, wrong song), for Admin > Library > Problems. One per person
-- and song; reporting again replaces it.
CREATE TABLE song_reports (
    track_id   INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    user_id    INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    reason     TEXT    NOT NULL,
    note       TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (track_id, user_id)
);
