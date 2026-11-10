-- The song list can show only songs with lyrics, or with timed lyrics.
CREATE INDEX lyrics_words ON lyrics (track_id) WHERE synced IS NOT NULL OR plain IS NOT NULL;
CREATE INDEX lyrics_timed ON lyrics (track_id) WHERE synced IS NOT NULL;
