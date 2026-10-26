-- Every artist credited on a track, so collaborations ("A & B") and
-- featured artists ("feat. C") each count as their own artist.
-- tracks.artist_id stays as the first credited artist.
CREATE TABLE track_artists (
    track_id  INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES artists (id),
    position  INTEGER NOT NULL,
    featured  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (track_id, artist_id)
);
CREATE INDEX track_artists_artist ON track_artists (artist_id);

-- Existing tracks get their current artist until the next scan re-reads
-- them (the scanner version bump makes that happen at startup).
INSERT OR IGNORE INTO track_artists (track_id, artist_id, position, featured)
SELECT id, artist_id, 0, 0 FROM tracks WHERE artist_id IS NOT NULL;
