-- Nodes: other DJINN boxes that play a room's music on their own speakers,
-- taking everything from this server (Main). A node asks to join; an admin
-- accepts with their password. Only a hash of the node's secret is kept.
-- Kicking deletes the row, which shuts the node out (and has it wipe its
-- cache).
CREATE TABLE nodes (
    id           INTEGER PRIMARY KEY,
    -- The node's own id, made on the node.
    node_id      TEXT    NOT NULL UNIQUE,
    name         TEXT    NOT NULL,
    -- SHA-256 of the node's secret, hex.
    secret_hash  TEXT    NOT NULL,
    status       TEXT    NOT NULL CHECK (status IN ('pending', 'approved')),
    room_id      INTEGER NOT NULL DEFAULT 1,
    address      TEXT,
    version      TEXT,
    platform     TEXT,
    requested_at INTEGER NOT NULL,
    approved_at  INTEGER,
    approved_by  INTEGER REFERENCES users (id) ON DELETE SET NULL,
    last_seen    INTEGER
);
