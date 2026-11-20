"""Lyric parsing, cleaning, AI notice handling, and DJINN safety validation."""

import html
import re
from typing import Optional

_EXCERPT_NOTICES: tuple[str, ...] = (
    "we are not licensed to display the full lyrics",
    "unfortunately, we are not authorized",
    "lyrics for this song have yet to be released",
    "暂无歌词",
    "no lyrics available",
    "lyrics not available",
)

_INSTRUMENTAL_MARKERS: set[str] = {
    "instrumental",
    "此歌曲为没有填词的纯音乐",
    "此歌曲为没有填词的纯音乐，请您欣赏",
    "纯音乐，请欣赏",
    "纯音乐,请欣赏",
    "纯音乐, 请欣赏",
    "纯音乐",
    "no lyrics",
    "music only",
    "this song is an instrumental",
    "instrumental track",
}

_NETEASE_CREDIT_RE = re.compile(
    r"^(?:"
    r"作词|作曲|编曲|制作人|制作|词|曲|唱|原唱|翻唱|演唱|歌手|"
    r"混音师|混音室|混音|母带处理|母带工程师|母带|和声|和音|"
    r"木吉他|电吉他|吉他|贝斯|鼓|打击乐|弦乐|键盘|钢琴|合成器|"
    r"口琴|萨克斯|小号|长笛|管乐|"
    r"录音师|录音室|录音|联合出品|出品人|出品|监制|统筹|企划|宣传|发行|"
    r"文案|封面|美工|海报|后期|音乐制作|特别鸣谢|歌词贡献者|翻译贡献者|"
    r"OP|SP|"
    r"Lyricist|Composer|Arranger|Producer|Mixed\s+by|Mastered\s+by|"
    r"Recorded\s+by|Written\s+by"
    r")\s*[:：].*",
    re.IGNORECASE,
)

_METADATA_TAG_PREFIXES: tuple[str, ...] = (
    "ar:",
    "ti:",
    "al:",
    "au:",
    "length:",
    "by:",
    "re:",
    "ve:",
)

AI_GENERATED_NOTICE = "(AI-generated lyrics)"
AI_SYNCED_NOTICE = "(AI-synced lyrics)"

_AI_NOTICE_RE = re.compile(
    r"^\((?:AI-generated lyrics|AI-synced lyrics)\)$",
    re.IGNORECASE,
)

_SECTION_MARKER_RE = re.compile(
    r"^\[(?:intro|verse|pre-chorus|chorus|post-chorus|bridge|hook|refrain|outro|interlude|solo|break|instrumental)(?:\s+[^\]]*)?\]$",
    re.IGNORECASE,
)

_SCRIPT_STYLE_RE = re.compile(
    r"<\s*(script|style|noscript)\b[^>]*>.*?<\s*/\s*\1\s*>",
    re.IGNORECASE | re.DOTALL,
)
_WIKI_TEMPLATE_RE = re.compile(r"\{\{.*?\}\}", re.DOTALL)
_BLOCK_BREAK_RE = re.compile(
    r"<\s*(?:br\s*/?|/?\s*(?:p|div)(?:\s[^>]*)?)>",
    re.IGNORECASE,
)
_HTML_TAG_RE = re.compile(
    r"<(?:/?[a-zA-Z][a-zA-Z0-9]*(?:\s[^>]*)?|!--.*?--)>",
    re.DOTALL,
)

MAX_ENTRY_BYTES = 200 * 1024

_SQL_PATTERNS: tuple[re.Pattern[str], ...] = (
    re.compile(
        r";\s*(?:DROP|DELETE|INSERT|UPDATE|ALTER|CREATE|ATTACH|PRAGMA|VACUUM|SELECT)\b",
        re.IGNORECASE,
    ),
    re.compile(r"\bUNION\s+(?:ALL\s+)?SELECT\b", re.IGNORECASE),
    re.compile(
        r"\bINSERT\s+INTO\s+[a-zA-Z_]\w*[^\n]*(?:\bVALUES\b|\bSELECT\b)",
        re.IGNORECASE,
    ),
    re.compile(
        r"\bDROP\s+(?:TABLE|DATABASE|INDEX|VIEW|TRIGGER)\b",
        re.IGNORECASE,
    ),
    re.compile(
        r"\bDELETE\s+FROM\s+[a-zA-Z_]\w*[^\n]*(?:\bWHERE\b|;)",
        re.IGNORECASE,
    ),
    re.compile(
        r"\bUPDATE\s+[a-zA-Z_]\w*\s+SET\b[^\n]*(?:\bWHERE\b|=)",
        re.IGNORECASE,
    ),
    re.compile(
        r"\bSELECT\s+(?:\*|\d+|COUNT\s*\([^)]*\)|[a-zA-Z_]\w*(?:\s*,\s*[a-zA-Z_]\w*)+)\s+FROM\s+[a-zA-Z_]\w*",
        re.IGNORECASE,
    ),
    re.compile(
        r"\bSELECT\s+[a-zA-Z_]\w*\s+FROM\s+[a-zA-Z_]\w*(?:\s+(?:WHERE|JOIN|ORDER\s+BY|GROUP\s+BY|LIMIT)\b|\s*;|\s*--)",
        re.IGNORECASE,
    ),
    re.compile(r"'\s*--"),
    re.compile(r"/\*.*?\*/", re.DOTALL),
)

_MARKUP_PATTERNS: tuple[re.Pattern[str], ...] = (
    re.compile(r"<\s*script\b", re.IGNORECASE),
    re.compile(r"<\s*iframe\b", re.IGNORECASE),
    re.compile(r"<\s*object\b", re.IGNORECASE),
    re.compile(r"<\s*embed\b", re.IGNORECASE),
    re.compile(r"<\s*svg\b[^>]*\bon\w+\s*=", re.IGNORECASE),
    re.compile(r"javascript\s*:", re.IGNORECASE),
    re.compile(r"data\s*:\s*text/html", re.IGNORECASE),
    re.compile(r"\bonerror\s*=", re.IGNORECASE),
    re.compile(r"<\?php\b", re.IGNORECASE),
    re.compile(r"\{\{"),
    re.compile(r"\$\{"),
    re.compile(r"<%"),
)

_SHELL_PATTERNS: tuple[re.Pattern[str], ...] = (
    re.compile(r"\$\([^)\n]+\)"),
    re.compile(r"(?<!\w)`[a-zA-Z0-9_./][a-zA-Z0-9_./ -]{1,}`(?!\w)"),
    re.compile(r"\brm\s+-rf\b", re.IGNORECASE),
    re.compile(r"\bpowershell\b", re.IGNORECASE),
    re.compile(r"\bcmd\.exe\b", re.IGNORECASE),
    re.compile(r"\bcurl\b[^\n]*\|\s*(?:ba)?sh\b", re.IGNORECASE),
    re.compile(r"\beval\s*\(", re.IGNORECASE),
    re.compile(r"\bexec\s*\(", re.IGNORECASE),
    re.compile(r"/etc/passwd"),
)


def parse_timestamp(tag: str) -> Optional[int]:
    """Parse `mm:ss`, `mm:ss.x`, `mm:ss.xx`, `mm:ss.xxx`, or `mm:ss:xx` into ms."""
    if ":" not in tag:
        return None
    minutes_str, rest = tag.split(":", 1)
    minutes_str = minutes_str.strip()
    if not minutes_str.isdigit():
        return None
    minutes = int(minutes_str)

    sep_idx = -1
    for sep in (".", ":"):
        idx = rest.find(sep)
        if idx != -1 and (sep_idx == -1 or idx < sep_idx):
            sep_idx = idx

    if sep_idx != -1:
        seconds_str = rest[:sep_idx]
        fraction_str = rest[sep_idx + 1 :]
    else:
        seconds_str = rest
        fraction_str = ""

    # NetEase extended timestamps can end in `-N` (e.g. `00:00.00-1`).
    if "-" in fraction_str:
        frac_main, frac_suffix = fraction_str.split("-", 1)
        if not frac_suffix.isdigit():
            return None
        fraction_str = frac_main

    seconds_str = seconds_str.strip()
    if not seconds_str.isdigit():
        return None
    seconds = int(seconds_str)
    if seconds >= 60:
        return None
    if fraction_str and (not fraction_str.isdigit() or len(fraction_str) > 3):
        return None

    if len(fraction_str) == 0:
        fraction_ms = 0
    elif len(fraction_str) == 1:
        fraction_ms = int(fraction_str) * 100
    elif len(fraction_str) == 2:
        fraction_ms = int(fraction_str) * 10
    else:
        fraction_ms = int(fraction_str)

    return minutes * 60_000 + seconds * 1000 + fraction_ms


def format_timestamp(ms: int) -> str:
    """Format milliseconds into canonical `[mm:ss.xx]` LRC timestamp."""
    clamped = max(0, int(ms))
    total_cs = (clamped + 5) // 10
    minutes = total_cs // 6000
    rem = total_cs % 6000
    seconds = rem // 100
    cs = rem % 100
    return f"[{minutes:02d}:{seconds:02d}.{cs:02d}]"


def leading_tags(line: str) -> tuple[list[str], str]:
    """Split leading `[...]` tags from a line's words."""
    tags: list[str] = []
    rest = line.lstrip()
    while rest.startswith("["):
        end = rest.find("]")
        if end == -1:
            break
        tags.append(rest[1:end])
        rest = rest[end + 1 :].lstrip()
    return tags, rest


def strip_word_timings(words: str) -> str:
    """Remove enhanced LRC `<mm:ss.xx>` word timings while keeping other `<...>` text."""
    out: list[str] = []
    rest = words
    while "<" in rest:
        start = rest.find("<")
        end = rest.find(">", start + 1)
        if end != -1 and parse_timestamp(rest[start + 1 : end]) is not None:
            out.append(rest[:start])
            rest = rest[end + 1 :]
        else:
            out.append(rest[: start + 1])
            rest = rest[start + 1 :]
    out.append(rest)
    return " ".join("".join(out).split())


def is_metadata_tag(tag: str) -> bool:
    """True for standard non-timed LRC metadata tags (`ar:`, `ti:`, `al:`, etc.)."""
    lower = tag.strip().lower()
    return any(lower.startswith(prefix) for prefix in _METADATA_TAG_PREFIXES)


def is_ai_notice_line(line: str) -> bool:
    """True when a line (with or without timestamp) is a listener AI notice."""
    _, words = leading_tags(line.strip())
    return bool(_AI_NOTICE_RE.match(words.strip()))


def is_timed(text: str) -> bool:
    """True when at least one line starts with a valid LRC timestamp."""
    for raw in text.splitlines():
        tags, _ = leading_tags(raw)
        if any(parse_timestamp(tag) is not None for tag in tags):
            return True
    return False


def parse_lrc(text: str) -> list[tuple[int, str]]:
    """Parse timed LRC lines into sorted `(ms, text)` pairs."""
    offset = 0
    lines: list[tuple[int, str]] = []
    for raw in text.splitlines():
        tags, words = leading_tags(raw)
        times: list[int] = []
        for tag in tags:
            ms = parse_timestamp(tag)
            if ms is not None:
                times.append(ms)
            elif tag.lower().startswith("offset:"):
                raw_off = tag.split(":", 1)[1].strip()
                try:
                    offset = int(raw_off)
                except ValueError:
                    pass
        if not times:
            continue
        cleaned_words = strip_word_timings(words)
        for ms in times:
            lines.append((ms, cleaned_words))

    adjusted = [(max(0, ms - offset), words) for ms, words in lines]
    adjusted.sort(key=lambda item: item[0])
    return adjusted


def plain_of(text: str) -> str:
    """Extract plain words from LRC text without timestamps or metadata tags."""
    out: list[str] = []
    for raw in text.splitlines():
        tags, words = leading_tags(raw)
        timed = any(parse_timestamp(tag) is not None for tag in tags)
        if not timed and tags:
            continue
        cleaned = strip_word_timings(words)
        if cleaned and _NETEASE_CREDIT_RE.match(cleaned.strip()):
            continue
        out.append(cleaned)
    return "\n".join(out).strip()


def extract_clean_lines(text: str, *, keep_ai_notices: bool = False) -> list[str]:
    """Extract ordered lyric lines from plain or LRC text for comparison/sync."""
    text = text.lstrip("\ufeff").replace("\r\n", "\n").replace("\r", "\n")
    lines: list[str] = []
    if is_timed(text):
        for _, words in parse_lrc(text):
            cleaned = words.strip()
            if not cleaned:
                continue
            if not keep_ai_notices and _AI_NOTICE_RE.match(cleaned):
                continue
            if _SECTION_MARKER_RE.match(cleaned) or _NETEASE_CREDIT_RE.match(cleaned):
                continue
            lines.append(cleaned)
    else:
        for raw in text.splitlines():
            stripped = raw.strip()
            if not stripped:
                continue
            tags, words = leading_tags(stripped)
            if tags and not words and all(
                is_metadata_tag(t) or t.lower().startswith("offset:") for t in tags
            ):
                continue
            if _SECTION_MARKER_RE.match(stripped):
                continue
            text_part = words if tags else stripped
            cleaned = strip_word_timings(text_part).strip()
            if not cleaned:
                continue
            if not keep_ai_notices and _AI_NOTICE_RE.match(cleaned):
                continue
            if _NETEASE_CREDIT_RE.match(cleaned):
                continue
            lines.append(cleaned)
    return lines


def is_garbage_lyrics(text: str) -> bool:
    """True when text is empty, an excerpt notice, or credit-only junk."""
    stripped = text.lstrip("\ufeff").strip()
    if not stripped:
        return True
    lower = stripped.lower()
    if any(notice in lower for notice in _EXCERPT_NOTICES):
        return True
    if is_instrumental_text(stripped):
        return False
    lines = extract_clean_lines(stripped)
    non_ws = sum(len(ln.replace(" ", "").replace("\t", "")) for ln in lines)
    if not lines or non_ws == 0:
        return True
    if len(lines) < 2 and non_ws < 12:
        return True
    return False


def clean_html_and_entities(text: str) -> Optional[str]:
    """Strip HTML/scripts/wiki templates and decode entities."""
    if not text:
        return None
    text = text.lstrip("\ufeff")
    lower = text.lower()
    if any(notice in lower for notice in _EXCERPT_NOTICES):
        return None

    cleaned = _SCRIPT_STYLE_RE.sub("", text)
    cleaned = _WIKI_TEMPLATE_RE.sub("", cleaned)
    cleaned = _BLOCK_BREAK_RE.sub("\n", cleaned)
    cleaned = _HTML_TAG_RE.sub("", cleaned)
    cleaned = html.unescape(cleaned)
    cleaned = cleaned.replace("[]", "")

    lines: list[str] = []
    for raw_line in cleaned.splitlines():
        line = raw_line.rstrip()
        if not line and (not lines or not lines[-1]):
            continue
        lines.append(line)
    while lines and not lines[-1]:
        lines.pop()

    joined = "\n".join(lines)
    return joined if joined.strip() else None


def is_instrumental_text(text: str) -> bool:
    """True if stripped text only says the track is instrumental."""
    stripped = text.lstrip("\ufeff").strip()
    if not stripped:
        return False

    non_empty_lines: list[str] = []
    for raw in stripped.splitlines():
        tags, words = leading_tags(raw)
        has_time = any(parse_timestamp(tag) is not None for tag in tags)
        if not has_time and tags:
            non_meta = [
                t
                for t in tags
                if not is_metadata_tag(t) and not t.lower().startswith("offset:")
            ]
            if not words and non_meta:
                words = " ".join(non_meta)
            else:
                continue
        clean_words = strip_word_timings(words).strip("[]() ").lower()
        if clean_words:
            non_empty_lines.append(clean_words)

    if not non_empty_lines:
        return False
    if len(non_empty_lines) <= 2:
        joined = " ".join(non_empty_lines)
        if joined in _INSTRUMENTAL_MARKERS:
            return True
    return False


def check_safety(text: str) -> Optional[str]:
    """Return a rule name if text looks like code/markup/binary rather than lyrics."""
    encoded = text.encode("utf-8", errors="replace")
    if len(encoded) > MAX_ENTRY_BYTES:
        return "too_large"

    if text.startswith("MZ") or text.startswith("\x7fELF") or text.startswith("#!"):
        return "executable_header"

    if "\x00" in text:
        return "binary_or_control"
    for ch in text:
        code = ord(ch)
        if code < 32 and ch not in ("\t", "\n", "\r"):
            return "binary_or_control"

    for pat in _SQL_PATTERNS:
        if pat.search(text):
            return "sql_injection"

    for pat in _MARKUP_PATTERNS:
        if pat.search(text):
            return "script_or_markup"

    for pat in _SHELL_PATTERNS:
        if pat.search(text):
            return "shell_command"

    return None
