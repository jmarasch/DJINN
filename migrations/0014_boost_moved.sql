-- Whether a backing actually moved the song up (it can't pass the first
-- place). Stepping back undoes that move and forgets the boost, so backing,
-- stepping back and backing again comes out even.
ALTER TABLE request_boosts ADD COLUMN moved INTEGER NOT NULL DEFAULT 0;
