-- A node can play to more than one speaker. The first speaker stays in
-- `device`, `delay_ms` and `trim`; the others are a JSON list of
-- {"device", "delay_ms", "trim"}, empty for none.
ALTER TABLE nodes ADD COLUMN extra_outputs TEXT NOT NULL DEFAULT '';
