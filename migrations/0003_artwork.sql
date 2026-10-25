-- Artwork sources, so album and artist images can be chosen after a scan:
-- albums prefer the first listed song's embedded cover, then the album
-- folder's image; artists use an image in their own folder, then an album
-- cover.
ALTER TABLE tracks ADD COLUMN embedded_art TEXT;
ALTER TABLE tracks ADD COLUMN folder_art TEXT;
ALTER TABLE tracks ADD COLUMN artist_art TEXT;
ALTER TABLE artists ADD COLUMN art_hash TEXT;
