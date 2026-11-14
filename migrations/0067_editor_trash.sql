-- When the library editor's trash is emptied, what went there can't be put
-- back: those history rows say so.
ALTER TABLE file_edits ADD COLUMN final_at INTEGER;
