-- Songs an admin asked the AI listener for from the library editor: make
-- lyrics ('listen') or time the words it has ('sync'). Workers take these
-- first, whatever the AI page's own choices, and the row goes when the work
-- is done.
CREATE TABLE ai_asked (
    track_id INTEGER PRIMARY KEY REFERENCES tracks (id) ON DELETE CASCADE,
    kind     TEXT    NOT NULL CHECK (kind IN ('listen', 'sync')),
    asked_at INTEGER NOT NULL
);

-- What to ask the AI for once an import's file is in the library.
ALTER TABLE import_files ADD COLUMN ai_want TEXT CHECK (ai_want IN ('listen', 'sync'));
