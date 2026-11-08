-- A node can stop correcting itself against Main's clock: it lines up once
-- when music starts and then plays straight through, without the speed nudges
-- and small skips that keep it in step. For telling whether those are heard.
ALTER TABLE nodes ADD COLUMN free_run INTEGER NOT NULL DEFAULT 0;
