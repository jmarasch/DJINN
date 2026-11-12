-- "Skip and play less": a skip that also counts against the song in the
-- auto mix and radio. A plain skip no longer does (it may only mean someone
-- wanted the next song more), so the old skipped flags aren't carried over.
-- play_events.play_less is a level: 0 none, 1 someone chose "play less",
-- 2 the room's skip votes sent it away (counts double).
ALTER TABLE play_events ADD COLUMN play_less INTEGER NOT NULL DEFAULT 0;
ALTER TABLE go_plays ADD COLUMN play_less INTEGER NOT NULL DEFAULT 0;
