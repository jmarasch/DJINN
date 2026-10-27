-- What each song sounds like (djinn_engine::features), for "more like
-- this", similar artists, and song radio. `features` is little-endian
-- f32s; NULL with a version set means analyzed but nothing to describe
-- (silence, unreadable).
ALTER TABLE tracks ADD COLUMN features BLOB;
ALTER TABLE tracks ADD COLUMN features_version INTEGER;
ALTER TABLE tracks ADD COLUMN bpm REAL;
