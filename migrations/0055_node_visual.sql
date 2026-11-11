-- How a node's screen draws its visualizer: the styles DJINN GO for Android
-- has ('bars', 'ring', 'mirror', 'warp', 'tunnel', 'plasma', 'kaleidoscope',
-- 'nebula', 'stars').
-- The screen setting can now also be an order of pages to try, such as
-- 'lyrics,visual,tv'.
ALTER TABLE nodes ADD COLUMN visual TEXT NOT NULL DEFAULT 'bars';
