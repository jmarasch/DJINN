-- How a node's screen draws its visualizer: 'bars', 'ring' or 'mirror' (the
-- shapes DJINN GO for Android draws).
-- The screen setting can now also be an order of pages to try, such as
-- 'lyrics,visual,tv'.
ALTER TABLE nodes ADD COLUMN visual TEXT NOT NULL DEFAULT 'bars';
