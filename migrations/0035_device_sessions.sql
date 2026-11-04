-- Apps sign in with a token that only GO's port should accept, so a session
-- has to say which kind it is. Sessions from before this are told apart by
-- the name the app gives itself; anything else stays an ordinary session,
-- which still works everywhere it did.
ALTER TABLE sessions ADD COLUMN device INTEGER NOT NULL DEFAULT 0;
UPDATE sessions SET device = 1 WHERE user_agent = 'DJINN GO for Android';
