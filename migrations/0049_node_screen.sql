-- What a screen plugged into a node shows: 'tv', 'lyrics', 'status' (the
-- node's own page) or 'off' (blank).
ALTER TABLE nodes ADD COLUMN screen TEXT NOT NULL DEFAULT 'tv';
