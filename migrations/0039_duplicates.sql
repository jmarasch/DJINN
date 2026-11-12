-- The start of each song's audio fingerprint, taken during sound analysis
-- (its own version, so changing it doesn't make the features stale), for
-- finding copies of a song.
ALTER TABLE track_sound ADD COLUMN print BLOB;
ALTER TABLE track_sound ADD COLUMN print_version INTEGER;

-- Songs the admin said aren't copies of each other (a < b).
CREATE TABLE duplicate_dismissed (
    a INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    b INTEGER NOT NULL REFERENCES tracks (id) ON DELETE CASCADE,
    PRIMARY KEY (a, b)
);
