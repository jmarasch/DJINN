-- Rooms: each area with its own speakers can be its own party, with its own
-- queue, rules and skip votes. Everything starts in room 1, "Main", so an
-- install with one room works as before. A room linked to another plays
-- what that one plays (its speakers join it), Sonos-style.
CREATE TABLE rooms (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    linked_to  INTEGER REFERENCES rooms (id) ON DELETE SET NULL,
    position   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
INSERT INTO rooms (id, name, position, created_at) VALUES (1, 'Main', 0, unixepoch() * 1000);

-- No REFERENCES on these: SQLite can't add a column with one and a default
-- other than NULL. Deleting a room moves its speakers and people to Main
-- (djinn-store::rooms).
ALTER TABLE outputs ADD COLUMN room_id INTEGER NOT NULL DEFAULT 1;
ALTER TABLE play_events ADD COLUMN room_id INTEGER NOT NULL DEFAULT 1;
ALTER TABLE request_events ADD COLUMN room_id INTEGER NOT NULL DEFAULT 1;
-- The room each person is in.
ALTER TABLE users ADD COLUMN room_id INTEGER NOT NULL DEFAULT 1;

-- A song can be queued once per room, not once in all. SQLite can't change
-- a UNIQUE in place, so the queue is made again with the same ids. Dropping
-- it removes its backers too (ON DELETE CASCADE), so they wait in a copy
-- and go back after.
PRAGMA defer_foreign_keys = ON;
CREATE TABLE request_queue_copy AS SELECT id, track_id, user_id, added_at, position FROM request_queue;
CREATE TABLE request_backers_copy AS SELECT queue_id, user_id, added_at FROM request_backers;
DROP TABLE request_queue;

CREATE TABLE request_queue (
    id       INTEGER PRIMARY KEY,
    room_id  INTEGER NOT NULL DEFAULT 1,
    track_id INTEGER NOT NULL REFERENCES tracks (id),
    user_id  INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL,
    position INTEGER NOT NULL,
    UNIQUE (room_id, track_id)
);
INSERT INTO request_queue (id, room_id, track_id, user_id, added_at, position)
    SELECT id, 1, track_id, user_id, added_at, position FROM request_queue_copy;
INSERT OR IGNORE INTO request_backers (queue_id, user_id, added_at)
    SELECT queue_id, user_id, added_at FROM request_backers_copy;
DROP TABLE request_queue_copy;
DROP TABLE request_backers_copy;
CREATE INDEX request_queue_position ON request_queue (room_id, position);
CREATE INDEX request_queue_user ON request_queue (user_id);
CREATE INDEX play_events_room ON play_events (room_id, started_at);
