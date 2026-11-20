"""Shared data structures for the AI lyrics listener."""

from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True)
class SongQuery:
    """Metadata describing a song for LRC headers and reporting."""

    artist: str
    title: str
    album: Optional[str] = None
    album_artist: Optional[str] = None
    duration_ms: Optional[int] = None


@dataclass(frozen=True)
class ExistingLyrics:
    """Existing lyrics loaded from djinn.db or lyrics-scraper output."""

    source: str
    plain: Optional[str]
    synced: Optional[str]
    is_timed: bool

    @property
    def best_text(self) -> str:
        """Return synced LRC when present, otherwise plain text."""
        if self.synced and self.synced.strip():
            return self.synced
        return self.plain or ""


@dataclass(frozen=True)
class TrackCandidate:
    """Active track row read from DJINN's SQLite database with scraper context."""

    id: int
    path: str
    title: str
    artist: str
    album: Optional[str] = None
    album_artist: Optional[str] = None
    track_no: Optional[int] = None
    year: Optional[int] = None
    duration_ms: Optional[int] = None
    djinn_lyrics_source: Optional[str] = None
    djinn_instrumental: bool = False
    online_checked_at: Optional[int] = None
    scraper_status: Optional[str] = None
    existing_lyrics: Optional[ExistingLyrics] = None

    def to_query(self) -> SongQuery:
        return SongQuery(
            artist=self.artist,
            title=self.title,
            album=self.album,
            album_artist=self.album_artist,
            duration_ms=self.duration_ms,
        )


@dataclass(frozen=True)
class TimedWord:
    """A single heard word or token with start and end timestamps in milliseconds."""

    start_ms: int
    end_ms: int
    word: str
    probability: float = 1.0


@dataclass(frozen=True)
class TimedLine:
    """A timed lyric line or vocal phrase interval in milliseconds."""

    ms: int
    end_ms: int
    text: str


@dataclass(frozen=True)
class ListenTranscript:
    """Raw output produced by listening to an audio file."""

    lines: list[TimedLine]
    words: list[TimedWord]
    instrumental: bool
    model_name: str
    vocal_separated: bool
    language: Optional[str] = None


@dataclass(frozen=True)
class ComparisonReport:
    """Accuracy comparison between existing lyrics and AI-heard audio transcript."""

    track_id: int
    artist: str
    title: str
    existing_source: str
    similarity: float
    word_error_rate: float
    matched_words: int
    reference_words: int
    heard_words: int
    verdict: str
    synced_lines_created: int = 0


@dataclass
class ListenerResult:
    """Final tagged lyrics or comparison outcome ready for formatting."""

    tag_kind: str
    model_name: str
    synced_lrc: Optional[str] = None
    plain_text: Optional[str] = None
    instrumental: bool = False
    comparison: Optional[ComparisonReport] = None
    # For lyrics an AI wrote by listening: how sure the model was of the
    # words it heard (0 to 1, the median of its word probabilities).
    confidence: Optional[float] = None
