-- Where imported lyrics came from: the zip's name and when it was imported,
-- and, for lyrics an AI worked on, how well its timing matched the words.
ALTER TABLE lyrics ADD COLUMN ai_match INTEGER;
ALTER TABLE lyrics ADD COLUMN import_name TEXT;
ALTER TABLE lyrics ADD COLUMN imported_at INTEGER;
