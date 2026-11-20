-- Lyrics an AI made by listening to the song (speech to text with timing).
-- They are guesses: any other source ranks above them, and LRCLIB is still
-- asked about those songs. `ai_model` names what made them.
ALTER TABLE lyrics ADD COLUMN ai INTEGER NOT NULL DEFAULT 0;
ALTER TABLE lyrics ADD COLUMN ai_model TEXT;
CREATE INDEX lyrics_ai ON lyrics (track_id) WHERE ai = 1;
