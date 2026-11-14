-- Songs handed to an AI worker (a computer running the lyrics listener) to
-- make or time lyrics for. Songs waiting for a worker are worked out from the
-- lyrics themselves; a row here says a worker has the song, did it, or
-- failed. A song a worker holds for too long goes back to waiting.
CREATE TABLE ai_jobs (
    track_id    INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    -- 'listen': no lyrics yet, make them. 'sync': plain lyrics, time them.
    kind        TEXT    NOT NULL CHECK (kind IN ('listen', 'sync')),
    status      TEXT    NOT NULL CHECK (status IN ('working', 'done', 'failed')),
    worker      TEXT,
    taken_at    INTEGER,
    finished_at INTEGER,
    error       TEXT,
    attempts    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX ai_jobs_status ON ai_jobs (status);
