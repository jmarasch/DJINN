-- How many songs (playing and coming up) a node keeps copies of ahead of time.
ALTER TABLE nodes ADD COLUMN cache_ahead INTEGER NOT NULL DEFAULT 12;
