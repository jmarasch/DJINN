-- Accounts in this role may have songs converted (DJINN GO's optional
-- converting, which the admin turns on separately).
ALTER TABLE roles ADD COLUMN convert INTEGER NOT NULL DEFAULT 1;
