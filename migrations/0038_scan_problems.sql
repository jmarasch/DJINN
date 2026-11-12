-- Files a library check couldn't add, or added with unreadable tags, for
-- Admin > Library > Problems. One row per path. Kinds found while walking
-- the folders (unsupported, unreadable, bad_name, missing_folder) are
-- replaced by every check; `tags` rows are written when the file is read
-- and belong to a song in `tracks`.
CREATE TABLE scan_problems (
    path       TEXT    PRIMARY KEY,
    kind       TEXT    NOT NULL,
    detail     TEXT    NOT NULL DEFAULT '',
    noticed_at INTEGER NOT NULL
);
CREATE INDEX scan_problems_kind ON scan_problems (kind);
