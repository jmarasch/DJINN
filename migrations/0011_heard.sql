-- What the optional AI model (djinn-ai, CLAP) heard in each song: a
-- 512-number embedding as little-endian f32s. NULL with a version set
-- means it listened but there was nothing to hear (silence, unreadable).
ALTER TABLE tracks ADD COLUMN heard BLOB;
ALTER TABLE tracks ADD COLUMN heard_version INTEGER;
