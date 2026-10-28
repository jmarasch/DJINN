-- Who has already moved each queued song up a place. Backing a song moves
-- it up only the first time, so stepping back and backing again can't walk
-- it to the top. The requester counts too: handing a request over and then
-- backing it doesn't move it either.
CREATE TABLE request_boosts (
    queue_id INTEGER NOT NULL REFERENCES request_queue (id) ON DELETE CASCADE,
    user_id  INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (queue_id, user_id)
);
INSERT OR IGNORE INTO request_boosts (queue_id, user_id)
    SELECT id, user_id FROM request_queue;
INSERT OR IGNORE INTO request_boosts (queue_id, user_id)
    SELECT queue_id, user_id FROM request_backers;
