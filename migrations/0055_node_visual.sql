-- A node's own visualizer settings (JSON, as Admin > Visualizers saves the
-- house's in the `visuals` setting), or '' to follow the house's.
-- The screen setting can now also be an order of pages to try, such as
-- 'lyrics,visual,tv'.
ALTER TABLE nodes ADD COLUMN visual TEXT NOT NULL DEFAULT '';
