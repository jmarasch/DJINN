-- The library editor. Corrections can change a song's track and disc
-- number too (NULL keeps the file's).
ALTER TABLE tag_edits ADD COLUMN track_no INTEGER;
ALTER TABLE tag_edits ADD COLUMN disc_no INTEGER;
-- A cover chosen in DJINN: an image in the art cache by its hash, or 'none'
-- for no cover (NULL: the file's and the folder's).
ALTER TABLE tag_edits ADD COLUMN cover TEXT;

-- Every file the editor wrote, with what it said before (JSON), so a
-- change can be undone. `size` and `mtime` are the file's right after DJINN
-- wrote it: a file that still has them only had its tags changed by DJINN,
-- so its sound analysis and what the AI heard stay valid.
CREATE TABLE file_edits (
    id        INTEGER PRIMARY KEY,
    -- One approval: undone together.
    batch     INTEGER NOT NULL,
    track_id  INTEGER REFERENCES tracks (id) ON DELETE SET NULL,
    path      TEXT    NOT NULL,
    -- 'tags', 'cover' or 'rename' (before and after are the old and new path).
    kind      TEXT    NOT NULL DEFAULT 'tags',
    before    TEXT    NOT NULL,
    after     TEXT    NOT NULL,
    size      INTEGER NOT NULL,
    mtime     INTEGER NOT NULL,
    made_at   INTEGER NOT NULL,
    made_by   INTEGER REFERENCES users (id) ON DELETE SET NULL,
    undone_at INTEGER
);
CREATE INDEX file_edits_path ON file_edits (path, size, mtime);
CREATE INDEX file_edits_batch ON file_edits (batch);

DROP TRIGGER tracks_file_changed;
CREATE TRIGGER tracks_file_changed AFTER UPDATE OF size, mtime ON tracks
WHEN (old.size <> new.size OR old.mtime <> new.mtime)
 AND NOT EXISTS (SELECT 1 FROM file_edits f
                 WHERE f.path = new.path AND f.size = new.size AND f.mtime = new.mtime)
BEGIN
    UPDATE track_sound SET features_version = NULL WHERE track_id = new.id;
    DELETE FROM track_heard WHERE track_id = new.id;
END;
