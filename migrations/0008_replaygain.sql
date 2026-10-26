-- ReplayGain from tags (gain in dB, peak linear), and loudness DJINN
-- measured itself for files without tags.
ALTER TABLE tracks ADD COLUMN rg_track_gain REAL;
ALTER TABLE tracks ADD COLUMN rg_track_peak REAL;
ALTER TABLE tracks ADD COLUMN rg_album_gain REAL;
ALTER TABLE tracks ADD COLUMN rg_album_peak REAL;
ALTER TABLE tracks ADD COLUMN loudness_lufs REAL;
ALTER TABLE tracks ADD COLUMN loudness_peak REAL;
-- When loudness was measured, or found unmeasurable (Unix ms).
ALTER TABLE tracks ADD COLUMN loudness_checked_at INTEGER;
