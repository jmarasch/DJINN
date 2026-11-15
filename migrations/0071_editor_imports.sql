-- Albums brought into the library editor to work on before they go into
-- the library: each file waits in `<db dir>/imports/<batch>/` (never a
-- music folder) until it's added to the library or thrown away.
CREATE TABLE import_files (
    id       INTEGER PRIMARY KEY,
    -- One import (usually an album); files added together share it.
    batch    INTEGER NOT NULL,
    -- The name it was brought in with.
    name     TEXT    NOT NULL,
    -- Where it waits.
    path     TEXT    NOT NULL UNIQUE,
    size     INTEGER NOT NULL,
    added_at INTEGER NOT NULL,
    added_by INTEGER REFERENCES users (id) ON DELETE SET NULL
);
CREATE INDEX import_files_batch ON import_files (batch);
