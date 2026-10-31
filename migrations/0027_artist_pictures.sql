-- Pictures of artists for slideshows, from their Wikimedia Commons
-- category (found through MusicBrainz and Wikidata). Fetched the first
-- time an artist's pictures are wanted, and kept in the image cache.
ALTER TABLE artists ADD COLUMN commons_category TEXT;
ALTER TABLE artists ADD COLUMN pictures_checked_at INTEGER;

CREATE TABLE artist_pictures (
    artist_id  INTEGER NOT NULL REFERENCES artists (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL,
    art_hash   TEXT    NOT NULL,
    -- "Author (License)", shown with the picture.
    credit     TEXT    NOT NULL,
    -- The file's page on Commons.
    credit_url TEXT    NOT NULL,
    PRIMARY KEY (artist_id, position)
);
