-- How many people favorited a song, album or artist, counted for the
-- library lists.
CREATE INDEX go_likes_ref ON go_likes (kind, ref_id);
