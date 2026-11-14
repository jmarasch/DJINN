-- A file the editor replaced with another recording is new sound: only
-- files DJINN retagged (tags, covers, length headers, and their undos)
-- keep their analysis.
DROP TRIGGER tracks_file_changed;
CREATE TRIGGER tracks_file_changed AFTER UPDATE OF size, mtime ON tracks
WHEN (old.size <> new.size OR old.mtime <> new.mtime)
 AND NOT EXISTS (SELECT 1 FROM file_edits f
                 WHERE f.path = new.path AND f.size = new.size AND f.mtime = new.mtime
                   AND f.kind NOT IN ('replace', 'undo-replace'))
BEGIN
    UPDATE track_sound SET features_version = NULL WHERE track_id = new.id;
    DELETE FROM track_heard WHERE track_id = new.id;
END;
