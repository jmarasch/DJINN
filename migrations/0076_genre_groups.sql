-- Genre containers: similar genres grouped under one name ("Metal") when the
-- genre_containers setting is on. Nothing in the library changes; the names
-- are folded when shown. `auto` containers were made by Auto-group and are
-- replaced the next time it runs; a container someone made or changed is not.
CREATE TABLE genre_groups (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    auto       INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

-- Which container a genre is in. A row with no container is a genre someone
-- took out by hand, which Auto-group leaves alone.
CREATE TABLE genre_group_members (
    genre    TEXT PRIMARY KEY,
    group_id INTEGER REFERENCES genre_groups (id) ON DELETE CASCADE,
    pinned   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX genre_group_members_group ON genre_group_members (group_id);
