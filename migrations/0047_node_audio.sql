-- Each node's speaker settings, kept on Main and editable from Main's Admin
-- or the node's own page. `devices` is what the node last reported it has
-- (JSON), so Admin can offer a choice. `offline` is what a node does once
-- it loses Main and has played what it had: 'stop' or 'cached' (keep
-- playing songs from its cache).
ALTER TABLE nodes ADD COLUMN device TEXT;
ALTER TABLE nodes ADD COLUMN delay_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE nodes ADD COLUMN trim REAL NOT NULL DEFAULT 1.0;
ALTER TABLE nodes ADD COLUMN equalizer TEXT;
ALTER TABLE nodes ADD COLUMN offline TEXT NOT NULL DEFAULT 'stop';
ALTER TABLE nodes ADD COLUMN devices TEXT;
