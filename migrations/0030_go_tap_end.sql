-- A tap can also add a song to the very end of the queue. SQLite can't change
-- a CHECK in place, so the table is made again. 'queue' still means after the
-- song playing and what you lined up before: "play next".
CREATE TABLE go_prefs_new (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    tap     TEXT    NOT NULL DEFAULT 'play' CHECK (tap IN ('play', 'queue', 'end'))
);
INSERT INTO go_prefs_new (user_id, tap) SELECT user_id, tap FROM go_prefs;
DROP TABLE go_prefs;
ALTER TABLE go_prefs_new RENAME TO go_prefs;
