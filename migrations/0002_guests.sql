-- Guest mode: people join with just a name. Each guest device gets a user
-- row with role 'guest' (identified by MAC address when it can be found),
-- so sessions, request limits, and history work the same as for accounts.

-- The name shown in the queue. Accounts leave it NULL and show username.
ALTER TABLE users ADD COLUMN display_name TEXT;
-- Exempt from request limits (set by the admin).
ALTER TABLE users ADD COLUMN unlimited INTEGER NOT NULL DEFAULT 0;
-- Hardware address of a guest's device, when known. Lets a returning
-- device keep its identity even if its cookie is gone.
ALTER TABLE users ADD COLUMN mac TEXT;
CREATE UNIQUE INDEX users_mac ON users (mac) WHERE mac IS NOT NULL;

-- Every time a device joins as a guest: which name it used, from where.
CREATE TABLE guest_joins (
    id      INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users (id) ON DELETE SET NULL,
    name    TEXT    NOT NULL,
    mac     TEXT,
    ip      TEXT,
    at      INTEGER NOT NULL
);
CREATE INDEX guest_joins_mac ON guest_joins (mac);

-- Where each request came from.
ALTER TABLE request_events ADD COLUMN mac TEXT;
ALTER TABLE request_events ADD COLUMN ip TEXT;
-- The name the requester went by at the time.
ALTER TABLE request_events ADD COLUMN name TEXT;
CREATE INDEX request_events_at ON request_events (at);
