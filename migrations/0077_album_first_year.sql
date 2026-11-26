-- The year an album first came out, from its MusicBrainz release group (the
-- tags often give a reissue's or remaster's year). The artist page shows and
-- sorts by it. Albums already found on MusicBrainz are looked up once more
-- (at the next start, while lookups are on) to fill it.
ALTER TABLE albums ADD COLUMN first_year INTEGER;
UPDATE albums SET info_checked_at = NULL WHERE mbid IS NOT NULL;
