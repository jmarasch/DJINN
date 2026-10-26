-- People who asked for a song that was already queued ("me too"). Each
-- backing uses one of that person's request slots and moves the song up a
-- place. The original requester is in request_queue, not here.
CREATE TABLE request_backers (
    queue_id INTEGER NOT NULL REFERENCES request_queue (id) ON DELETE CASCADE,
    user_id  INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (queue_id, user_id)
);
CREATE INDEX request_backers_user ON request_backers (user_id);
