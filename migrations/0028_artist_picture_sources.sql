-- Distinguish Commons downloads from pictures licensed and added by the host.
ALTER TABLE artist_pictures ADD COLUMN source TEXT NOT NULL DEFAULT 'commons';
-- Remote images are displayed from their provider, never saved in art/.
ALTER TABLE artist_pictures ADD COLUMN remote_url TEXT;
