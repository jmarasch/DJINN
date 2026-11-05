-- Plays on GO count toward what's popular, which asks by song.
CREATE INDEX go_plays_track ON go_plays (track_id, skipped);
