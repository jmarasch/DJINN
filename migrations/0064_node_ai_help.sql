-- Nodes helping with AI lyrics. Each node is switched on by the admin
-- (ai_help), with a Whisper model chosen for it (NULL: the one suggested
-- from what the node reported, ai_report as JSON: its processor, memory,
-- Python, install state and measured speed).
ALTER TABLE nodes ADD COLUMN ai_help INTEGER NOT NULL DEFAULT 0;
ALTER TABLE nodes ADD COLUMN ai_model TEXT;
ALTER TABLE nodes ADD COLUMN ai_report TEXT;

-- A song can also be 'prepared': Main has isolated its vocals into a small
-- file for a node to listen to. SQLite can't change a CHECK, so the table
-- is made again.
CREATE TABLE ai_jobs_new (
    track_id    INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL CHECK (kind IN ('listen', 'sync')),
    status      TEXT    NOT NULL CHECK (status IN ('working', 'prepared', 'done', 'failed')),
    worker      TEXT,
    taken_at    INTEGER,
    finished_at INTEGER,
    error       TEXT,
    attempts    INTEGER NOT NULL DEFAULT 0
);
INSERT INTO ai_jobs_new SELECT track_id, kind, status, worker, taken_at, finished_at, error, attempts FROM ai_jobs;
DROP TABLE ai_jobs;
ALTER TABLE ai_jobs_new RENAME TO ai_jobs;
CREATE INDEX ai_jobs_status ON ai_jobs (status);
