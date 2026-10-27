-- Songs identified by audio fingerprint (AcoustID + MusicBrainz).
-- status: 'found' (waiting for the admin), 'applied' (fills in what the
-- tags lack, on every scan), 'dismissed', 'none' (no trustworthy match).
-- Files are never changed.
CREATE TABLE track_ids (
    track_id   INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    checked_at INTEGER NOT NULL,
    status     TEXT    NOT NULL,
    score      REAL,
    recording  TEXT,
    title      TEXT,
    artist     TEXT,
    album      TEXT,
    year       INTEGER,
    genres     TEXT
);
CREATE INDEX track_ids_status ON track_ids (status);
