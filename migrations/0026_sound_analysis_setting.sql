-- Sound analysis became a switch, off for new installs. Installs that
-- already analyzed songs keep it on.
INSERT INTO settings (key, value)
SELECT 'sound_analysis', '1'
WHERE EXISTS (SELECT 1 FROM tracks WHERE features_version IS NOT NULL)
ON CONFLICT (key) DO NOTHING;
