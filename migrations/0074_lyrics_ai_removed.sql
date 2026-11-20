-- Set when an admin removed lyrics an AI made or timed as bad: the song keeps
-- a red microphone, and the AI doesn't take it up again until an admin allows
-- its lyrics again.
ALTER TABLE lyrics ADD COLUMN ai_removed INTEGER NOT NULL DEFAULT 0;
