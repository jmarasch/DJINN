"""LRC formatting with DJINN import headers and AI provenance tags."""

from .cleaner import (
    AI_GENERATED_NOTICE,
    AI_SYNCED_NOTICE,
    check_safety,
    format_timestamp,
    is_ai_notice_line,
    is_metadata_tag,
    is_timed,
    leading_tags,
    parse_lrc,
)
from .models import ListenerResult, SongQuery, TimedLine


def _sanitize_tag_value(value: str) -> str:
    """Keep LRC header values single-line and safe for `[tag:value]` parsing."""
    cleaned = (
        value.replace("\r", " ")
        .replace("\n", " ")
        .replace("[", "(")
        .replace("]", ")")
    )
    return " ".join(cleaned.split())


def _strip_existing_headers_and_notices(body: str) -> str:
    """Strip pre-existing metadata header lines and AI notice lines."""
    out: list[str] = []
    for raw in body.splitlines():
        tags, words = leading_tags(raw)
        if tags and not words and all(is_metadata_tag(t) or t.lower().startswith("offset:") for t in tags):
            continue
        if is_ai_notice_line(raw):
            continue
        out.append(raw)
    return "\n".join(out).strip()


def lines_to_lrc_body(lines: list[TimedLine]) -> str:
    """Format a sequence of TimedLine items into sorted `[mm:ss.xx]text` lines."""
    cleaned: list[tuple[int, str]] = []
    for line in lines:
        text = " ".join(line.text.replace("\r", " ").replace("\n", " ").split())
        if not text:
            continue
        cleaned.append((max(0, int(line.ms)), text))
    cleaned.sort(key=lambda item: item[0])
    return "\n".join(f"{format_timestamp(ms)}{text}" for ms, text in cleaned)


def notice_for_kind(tag_kind: str) -> str:
    """Return the visible player notice text for a tag kind."""
    if tag_kind == "ai-synced":
        return AI_SYNCED_NOTICE
    return AI_GENERATED_NOTICE


def build_by_header(result: ListenerResult) -> str:
    """Build the `[by:...]` LRC metadata tag identifying AI provenance."""
    model = _sanitize_tag_value(result.model_name or "unknown")
    if result.tag_kind == "ai-synced":
        if result.comparison is not None and result.comparison.verdict != "timing_only":
            pct = int(round(max(0.0, min(1.0, result.comparison.similarity)) * 100))
            return f"[by:lyrics-listener (AI-synced; model={model}; match={pct}%)]"
        return f"[by:lyrics-listener (AI-synced; model={model})]"
    return f"[by:lyrics-listener (AI-generated; model={model})]"


def format_lrc_file(track: SongQuery, result: ListenerResult) -> str:
    """Format a ListenerResult into UTF-8 `.lrc` content with DJINN headers and AI tags."""
    if result.instrumental:
        raise ValueError("Cannot format an instrumental result as an LRC file")

    notice = notice_for_kind(result.tag_kind)

    headers: list[str] = []
    if track.artist:
        headers.append(f"[ar:{_sanitize_tag_value(track.artist)}]")
    if track.title:
        headers.append(f"[ti:{_sanitize_tag_value(track.title)}]")
    if track.album:
        headers.append(f"[al:{_sanitize_tag_value(track.album)}]")
    if track.duration_ms is not None and track.duration_ms > 0:
        total_sec = (track.duration_ms + 500) // 1000
        headers.append(f"[length:{total_sec // 60:02d}:{total_sec % 60:02d}]")
    headers.append(build_by_header(result))

    if result.synced_lrc and is_timed(result.synced_lrc):
        parsed = parse_lrc(result.synced_lrc)
        lyric_lines: list[tuple[int, str]] = []
        for ms, words in parsed:
            text = words.strip()
            if not text:
                continue
            if is_ai_notice_line(text):
                continue
            lyric_lines.append((max(0, ms), text))
        if not lyric_lines:
            raise ValueError("Synced LRC body has no lyric lines")
        lyric_lines.sort(key=lambda item: item[0])
        body_lines = [f"{format_timestamp(0)}{notice}"]
        body_lines.extend(f"{format_timestamp(ms)}{text}" for ms, text in lyric_lines)
        body = "\n".join(body_lines)
    elif result.plain_text and result.plain_text.strip():
        stripped_body = _strip_existing_headers_and_notices(result.plain_text)
        if not stripped_body:
            raise ValueError("Plain lyric body is empty after stripping headers")
        body = f"{notice}\n{stripped_body}"
    else:
        raise ValueError("ListenerResult contains neither synced nor plain text")

    content = "\n".join(headers) + "\n\n" + body + "\n"
    violation = check_safety(content)
    if violation is not None:
        raise ValueError(f"Generated LRC failed safety check: {violation}")
    return content
