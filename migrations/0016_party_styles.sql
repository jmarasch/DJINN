-- Party styles: separate memories of what each kind of party liked
-- ("Halloween", "Family BBQ"). Requests and plays count toward the style
-- in use when they happened; clearing a style's history sets style_id to
-- NULL, so the request log and recently played songs keep them.
CREATE TABLE party_styles (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    created_at INTEGER NOT NULL
);

-- Everything learned so far belongs to the first style.
INSERT INTO party_styles (id, name, created_at)
VALUES (1, 'Everyday', CAST(strftime('%s', 'now') AS INTEGER) * 1000);

ALTER TABLE request_events
    ADD COLUMN style_id INTEGER REFERENCES party_styles (id) ON DELETE SET NULL;
ALTER TABLE play_events
    ADD COLUMN style_id INTEGER REFERENCES party_styles (id) ON DELETE SET NULL;
UPDATE request_events SET style_id = 1;
UPDATE play_events SET style_id = 1;
CREATE INDEX request_events_style ON request_events (style_id, track_id);
CREATE INDEX play_events_style ON play_events (style_id, track_id);

INSERT OR IGNORE INTO settings (key, value) VALUES ('party_style', '1');
