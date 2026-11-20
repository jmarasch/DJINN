-- The Problems page's "vanished" check looks for a library song of the same
-- size for each song that went missing. Without this it read the whole
-- tracks table once per missing song.
CREATE INDEX tracks_size ON tracks (size);
