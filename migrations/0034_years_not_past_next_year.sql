-- A song's year can be next year at the latest, as with a release announced
-- ahead of time. Migration 33's upper limit was further off than that.
UPDATE tracks SET year = NULL WHERE year > CAST(strftime('%Y', 'now') AS INTEGER) + 1;
UPDATE albums SET year = NULL WHERE year > CAST(strftime('%Y', 'now') AS INTEGER) + 1;
