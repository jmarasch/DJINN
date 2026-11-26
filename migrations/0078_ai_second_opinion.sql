-- A song a node's smaller model wasn't sure of waits for this computer's
-- bigger one: a second opinion, taken only by this computer's listener.
ALTER TABLE ai_jobs ADD COLUMN second_opinion INTEGER NOT NULL DEFAULT 0;

-- Songs the AI listener finished without storing lyrics heard no singing:
-- they're instrumentals.
UPDATE lyrics SET instrumental = 1
 WHERE source = 'none' AND instrumental = 0 AND ai_removed = 0
   AND track_id IN (SELECT track_id FROM ai_jobs WHERE status = 'done' AND kind = 'listen');
