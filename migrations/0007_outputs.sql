-- Outputs: `device_name` holds the engine's device id, or '' for the
-- system default device, which always exists and can't be removed.
ALTER TABLE outputs ADD COLUMN label TEXT NOT NULL DEFAULT '';
ALTER TABLE outputs ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

INSERT OR IGNORE INTO outputs (device_name, label, enabled, volume_trim, delay_ms, position)
VALUES ('', 'System default', 1, 1.0, 0, 0);
