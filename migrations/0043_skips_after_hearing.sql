-- A song skipped within six hours of the same person hearing it all the way
-- through was just heard, not disliked (radio from a song that had just
-- ended started with it again). New skips like that aren't recorded
-- (`personal::HEARD_RECENTLY_MS`); this clears the ones already kept.
DELETE FROM go_plays
WHERE skipped = 1
  AND EXISTS (
    SELECT 1 FROM go_plays heard
    WHERE heard.user_id = go_plays.user_id
      AND heard.track_id = go_plays.track_id
      AND heard.skipped = 0
      AND heard.at BETWEEN go_plays.at - 21600000 AND go_plays.at
  );
