-- The artist list shows a short bio and genres, so a change to either
-- refreshes the kept lists.
DROP TRIGGER browse_artists_update;
CREATE TRIGGER browse_artists_update AFTER UPDATE OF name, art_hash, bio, genres ON artists
BEGIN UPDATE browse_version SET version = version + 1; END;
