-- Each person's DJINN GO preferences.
CREATE TABLE go_prefs (
    user_id INTEGER PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    -- What tapping a song does: 'play' it now, or add it to the 'queue'.
    tap     TEXT    NOT NULL DEFAULT 'play' CHECK (tap IN ('play', 'queue'))
);
