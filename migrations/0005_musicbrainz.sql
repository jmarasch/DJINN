-- MusicBrainz lookups (opt-in, needs a contact set by the host): exact
-- artist and album identities, genres, album covers from the Cover Art
-- Archive, and credits for free-licensed artist photos.
ALTER TABLE artists ADD COLUMN mbid TEXT;
ALTER TABLE artists ADD COLUMN genres TEXT;
ALTER TABLE artists ADD COLUMN photo_credit TEXT;
ALTER TABLE artists ADD COLUMN photo_credit_url TEXT;

ALTER TABLE albums ADD COLUMN mbid TEXT;
ALTER TABLE albums ADD COLUMN genres TEXT;
ALTER TABLE albums ADD COLUMN online_art_hash TEXT;
ALTER TABLE albums ADD COLUMN info_checked_at INTEGER;
