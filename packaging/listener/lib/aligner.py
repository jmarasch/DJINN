"""Accuracy comparison and sequence-alignment time syncing for lyrics."""

import re
import unicodedata
from array import array
from collections import Counter
from dataclasses import dataclass
from typing import Optional

from .cleaner import extract_clean_lines
from .models import ComparisonReport, ListenTranscript, TimedLine, TimedWord

_TOKEN_RE = re.compile(r"[^\W_]+(?:['\u2019`][^\W_]+)*", re.UNICODE)
_APOSTROPHES_RE = re.compile(r"['\u2019`]")

_WORD_ALIASES: dict[str, str] = {
    "cuz": "because",
    "cause": "because",
    "cos": "because",
    "til": "until",
    "till": "until",
    "ya": "you",
    "em": "them",
    "im": "iam",
}


@dataclass(frozen=True)
class _RefToken:
    line_idx: int
    pos_in_line: int
    norm: str


def normalize_word(token: str) -> str:
    """Normalize a single token for fuzzy lyric comparison."""
    norm = unicodedata.normalize("NFKC", token).lower()
    norm = _APOSTROPHES_RE.sub("", norm)
    return "".join(ch for ch in norm if ch.isalnum())


def tokenize_line(text: str) -> list[str]:
    """Split a lyric line into normalized alphanumeric tokens."""
    out: list[str] = []
    for raw in _TOKEN_RE.findall(text):
        tok = normalize_word(raw)
        if tok:
            out.append(tok)
    return out


def _levenshtein_str(a: str, b: str, max_dist: int = 2) -> int:
    if abs(len(a) - len(b)) > max_dist:
        return max_dist + 1
    if a == b:
        return 0
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, start=1):
        curr = [i]
        row_min = i
        for j, cb in enumerate(b, start=1):
            cost = 0 if ca == cb else 1
            val = min(prev[j] + 1, curr[j - 1] + 1, prev[j - 1] + cost)
            curr.append(val)
            if val < row_min:
                row_min = val
        if row_min > max_dist:
            return max_dist + 1
        prev = curr
    return prev[-1]


def _tokens_fuzzy_equal(a: str, b: str) -> bool:
    """True when two normalized words represent the same spoken word or minor variant."""
    if a == b:
        return True
    if _WORD_ALIASES.get(a, a) == _WORD_ALIASES.get(b, b):
        return True
    if len(a) >= 4 and len(b) >= 4:
        if (a.endswith("in") and b == a + "g") or (b.endswith("in") and a == b + "g"):
            return True
    if len(a) >= 5 and len(b) >= 5 and _levenshtein_str(a, b, max_dist=1) <= 1:
        return True
    return False


def _token_edit_distance(ref: list[str], hyp: list[str]) -> int:
    """Compute word-level Levenshtein edit distance between reference and hypothesis."""
    n = len(ref)
    m = len(hyp)
    if n == 0:
        return m
    if m == 0:
        return n

    vocab: dict[str, int] = {}
    ref_ids = [vocab.setdefault(w, len(vocab)) for w in ref]
    hyp_ids = [vocab.setdefault(w, len(vocab)) for w in hyp]

    # Use banded DP for extremely long songs to keep runtime predictable.
    if n * m > 2_000_000:
        band = abs(n - m) + 320
        inf = n + m + 1
        prev_row = {j: j for j in range(min(m, band) + 1)}
        for i in range(1, n + 1):
            center = (i * m) // max(1, n)
            j_lo = max(0, center - band)
            j_hi = min(m, center + band)
            curr_row: dict[int, int] = {}
            if j_lo == 0:
                curr_row[0] = i
            r_id = ref_ids[i - 1]
            for j in range(max(1, j_lo), j_hi + 1):
                sub_cost = 0 if r_id == hyp_ids[j - 1] else 1
                val = min(
                    prev_row.get(j, inf) + 1,
                    curr_row.get(j - 1, inf) + 1,
                    prev_row.get(j - 1, inf) + sub_cost,
                )
                curr_row[j] = val
            prev_row = curr_row
        return prev_row.get(m, abs(n - m))

    prev = list(range(m + 1))
    for i in range(1, n + 1):
        r_id = ref_ids[i - 1]
        curr = [i]
        curr_append = curr.append
        for j in range(1, m + 1):
            cost = 0 if r_id == hyp_ids[j - 1] else 1
            ins = curr[j - 1] + 1
            dele = prev[j] + 1
            sub = prev[j - 1] + cost
            curr_append(sub if sub <= ins and sub <= dele else (ins if ins <= dele else dele))
        prev = curr
    return prev[m]


def _align_token_sequences(
    ref_tokens: list[str],
    hyp_tokens: list[str],
) -> list[tuple[int, int]]:
    """Global Needleman-Wunsch alignment returning matched `(ref_idx, hyp_idx)` pairs."""
    n = len(ref_tokens)
    m = len(hyp_tokens)
    if n == 0 or m == 0:
        return []

    vocab: dict[str, int] = {}
    ref_ids = [vocab.setdefault(w, len(vocab)) for w in ref_tokens]
    hyp_ids = [vocab.setdefault(w, len(vocab)) for w in hyp_tokens]

    # Precompute fuzzy pairs among unique word IDs so DP runs over ints only.
    unique_ref = set(ref_ids)
    unique_hyp = set(hyp_ids)
    id_to_word = {vid: w for w, vid in vocab.items()}
    fuzzy_pairs: set[tuple[int, int]] = set()
    for rid in unique_ref:
        rw = id_to_word[rid]
        for hid in unique_hyp:
            if rid != hid and _tokens_fuzzy_equal(rw, id_to_word[hid]):
                fuzzy_pairs.add((rid, hid))

    match_exact = 3.0
    match_fuzzy = 1.5
    mismatch_pen = -1.5
    gap_pen = -1.0

    # Banded fallback for huge inputs.
    if n * m > 2_500_000:
        return _align_banded(
            ref_ids,
            hyp_ids,
            fuzzy_pairs,
            match_exact=match_exact,
            match_fuzzy=match_fuzzy,
            mismatch_pen=mismatch_pen,
            gap_pen=gap_pen,
        )

    # Traceback direction codes: 1 = diag, 2 = up (gap in hyp), 3 = left (gap in ref).
    tb: list[bytearray] = [bytearray(m + 1)]
    for j in range(1, m + 1):
        tb[0][j] = 3

    prev = [j * gap_pen for j in range(m + 1)]

    for i in range(1, n + 1):
        r_id = ref_ids[i - 1]
        row_tb = bytearray(m + 1)
        row_tb[0] = 2
        curr = array("d", [i * gap_pen])
        curr_append = curr.append

        for j in range(1, m + 1):
            h_id = hyp_ids[j - 1]
            if r_id == h_id:
                diag_score = prev[j - 1] + match_exact
            elif (r_id, h_id) in fuzzy_pairs:
                diag_score = prev[j - 1] + match_fuzzy
            else:
                diag_score = prev[j - 1] + mismatch_pen

            up_score = prev[j] + gap_pen
            left_score = curr[j - 1] + gap_pen

            if diag_score >= up_score and diag_score >= left_score:
                curr_append(diag_score)
                row_tb[j] = 1
            elif up_score >= left_score:
                curr_append(up_score)
                row_tb[j] = 2
            else:
                curr_append(left_score)
                row_tb[j] = 3

        tb.append(row_tb)
        prev = list(curr)

    pairs: list[tuple[int, int]] = []
    i = n
    j = m
    while i > 0 or j > 0:
        move = tb[i][j]
        if move == 1 and i > 0 and j > 0:
            rid = ref_ids[i - 1]
            hid = hyp_ids[j - 1]
            if rid == hid or (rid, hid) in fuzzy_pairs:
                pairs.append((i - 1, j - 1))
            i -= 1
            j -= 1
        elif move == 2 and i > 0:
            i -= 1
        elif j > 0:
            j -= 1
        else:
            break

    pairs.reverse()
    return pairs


def _align_banded(
    ref_ids: list[int],
    hyp_ids: list[int],
    fuzzy_pairs: set[tuple[int, int]],
    *,
    match_exact: float,
    match_fuzzy: float,
    mismatch_pen: float,
    gap_pen: float,
) -> list[tuple[int, int]]:
    """Banded global alignment when `len(ref) * len(hyp)` is very large."""
    n = len(ref_ids)
    m = len(hyp_ids)
    band = abs(n - m) + 350
    neg_inf = -1e12

    scores: dict[tuple[int, int], float] = {(0, 0): 0.0}
    back: dict[tuple[int, int], int] = {}

    for j in range(1, min(m, band) + 1):
        scores[(0, j)] = j * gap_pen
        back[(0, j)] = 3

    for i in range(1, n + 1):
        center = (i * m) // max(1, n)
        j_lo = max(0, center - band)
        j_hi = min(m, center + band)
        if j_lo == 0:
            scores[(i, 0)] = i * gap_pen
            back[(i, 0)] = 2
        r_id = ref_ids[i - 1]
        for j in range(max(1, j_lo), j_hi + 1):
            h_id = hyp_ids[j - 1]
            if r_id == h_id:
                sub = match_exact
            elif (r_id, h_id) in fuzzy_pairs:
                sub = match_fuzzy
            else:
                sub = mismatch_pen

            diag = scores.get((i - 1, j - 1), neg_inf) + sub
            up = scores.get((i - 1, j), neg_inf) + gap_pen
            left = scores.get((i, j - 1), neg_inf) + gap_pen
            if diag >= up and diag >= left:
                scores[(i, j)] = diag
                back[(i, j)] = 1
            elif up >= left:
                scores[(i, j)] = up
                back[(i, j)] = 2
            else:
                scores[(i, j)] = left
                back[(i, j)] = 3

    pairs: list[tuple[int, int]] = []
    i = n
    j = m
    while i > 0 or j > 0:
        move = back.get((i, j))
        if move is None:
            if i > 0 and j > 0:
                i -= 1
                j -= 1
            elif i > 0:
                i -= 1
            else:
                j -= 1
            continue
        if move == 1 and i > 0 and j > 0:
            rid = ref_ids[i - 1]
            hid = hyp_ids[j - 1]
            if rid == hid or (rid, hid) in fuzzy_pairs:
                pairs.append((i - 1, j - 1))
            i -= 1
            j -= 1
        elif move == 2 and i > 0:
            i -= 1
        elif j > 0:
            j -= 1
        else:
            break

    pairs.reverse()
    return pairs


def _transcript_timed_words(transcript: ListenTranscript) -> list[tuple[TimedWord, str]]:
    """Build normalized `(TimedWord, token)` list from transcript words or lines."""
    out: list[tuple[TimedWord, str]] = []
    if transcript.words:
        for tw in transcript.words:
            tokens = tokenize_line(tw.word)
            if not tokens:
                continue
            if len(tokens) == 1:
                out.append((tw, tokens[0]))
            else:
                span = max(0, tw.end_ms - tw.start_ms)
                step = span / len(tokens)
                for idx, tok in enumerate(tokens):
                    s_ms = int(round(tw.start_ms + idx * step))
                    e_ms = int(round(tw.start_ms + (idx + 1) * step))
                    out.append(
                        (
                            TimedWord(
                                start_ms=s_ms,
                                end_ms=max(s_ms, e_ms),
                                word=tok,
                                probability=tw.probability,
                            ),
                            tok,
                        )
                    )
        return out

    # Fallback: split line timestamps evenly across words when word_timestamps are absent.
    for line in transcript.lines:
        tokens = tokenize_line(line.text)
        if not tokens:
            continue
        span = max(200 * len(tokens), line.end_ms - line.ms)
        step = span / len(tokens)
        for idx, tok in enumerate(tokens):
            s_ms = int(round(line.ms + idx * step))
            e_ms = int(round(line.ms + (idx + 1) * step))
            out.append(
                (
                    TimedWord(start_ms=s_ms, end_ms=max(s_ms, e_ms), word=tok),
                    tok,
                )
            )
    return out


def compare_lyrics(
    existing_text: str,
    transcript: ListenTranscript,
    *,
    track_id: int = 0,
    artist: str = "",
    title: str = "",
    existing_source: str = "",
    min_similarity: float = 0.30,
    synced_lines_created: int = 0,
) -> ComparisonReport:
    """Compare existing lyrics against AI-heard transcript and compute accuracy."""
    ref_lines = extract_clean_lines(existing_text)
    ref_tokens = [tok for line in ref_lines for tok in tokenize_line(line)]
    heard_pairs = _transcript_timed_words(transcript)
    hyp_tokens = [tok for _, tok in heard_pairs]

    if transcript.model_name == "energy-onset" and not hyp_tokens:
        return ComparisonReport(
            track_id=track_id,
            artist=artist,
            title=title,
            existing_source=existing_source,
            similarity=0.0,
            word_error_rate=1.0,
            matched_words=0,
            reference_words=len(ref_tokens),
            heard_words=0,
            verdict="instrumental" if transcript.instrumental else "timing_only",
            synced_lines_created=synced_lines_created,
        )

    if transcript.instrumental and not hyp_tokens:
        return ComparisonReport(
            track_id=track_id,
            artist=artist,
            title=title,
            existing_source=existing_source,
            similarity=0.0,
            word_error_rate=1.0,
            matched_words=0,
            reference_words=len(ref_tokens),
            heard_words=0,
            verdict="instrumental",
            synced_lines_created=synced_lines_created,
        )

    if not ref_tokens or not hyp_tokens:
        verdict = "instrumental" if transcript.instrumental else "mismatch"
        return ComparisonReport(
            track_id=track_id,
            artist=artist,
            title=title,
            existing_source=existing_source,
            similarity=0.0,
            word_error_rate=1.0,
            matched_words=0,
            reference_words=len(ref_tokens),
            heard_words=len(hyp_tokens),
            verdict=verdict,
            synced_lines_created=synced_lines_created,
        )

    aligned_pairs = _align_token_sequences(ref_tokens, hyp_tokens)
    matched_words = len(aligned_pairs)
    edits = _token_edit_distance(ref_tokens, hyp_tokens)
    wer = round(edits / max(1, len(ref_tokens)), 4)

    seq_f1 = (2.0 * matched_words) / (len(ref_tokens) + len(hyp_tokens))
    if len(ref_tokens) >= 2 and len(hyp_tokens) >= 2:
        ref_bi = Counter(zip(ref_tokens, ref_tokens[1:]))
        hyp_bi = Counter(zip(hyp_tokens, hyp_tokens[1:]))
        bi_overlap = sum((ref_bi & hyp_bi).values())
        bi_f1 = (2.0 * bi_overlap) / (len(ref_tokens) - 1 + len(hyp_tokens) - 1)
        similarity = 0.70 * seq_f1 + 0.30 * bi_f1
    else:
        similarity = seq_f1

    similarity = round(max(0.0, min(1.0, similarity)), 4)

    if transcript.instrumental:
        verdict = "instrumental"
    elif similarity >= 0.60:
        verdict = "match"
    elif similarity >= min_similarity:
        verdict = "partial_match"
    else:
        verdict = "mismatch"

    return ComparisonReport(
        track_id=track_id,
        artist=artist,
        title=title,
        existing_source=existing_source,
        similarity=similarity,
        word_error_rate=wer,
        matched_words=matched_words,
        reference_words=len(ref_tokens),
        heard_words=len(hyp_tokens),
        verdict=verdict,
        synced_lines_created=synced_lines_created,
    )


def _estimate_ms_per_word(heard_words: list[tuple[TimedWord, str]]) -> float:
    durations: list[int] = []
    for tw, _ in heard_words:
        dur = tw.end_ms - tw.start_ms
        if 80 <= dur <= 900:
            durations.append(dur)
    if not durations:
        return 350.0
    durations.sort()
    mid = durations[len(durations) // 2]
    return float(max(180, min(600, mid)))


def _distribute_lines_across_intervals(
    ref_lines: list[str],
    line_weights: list[int],
    intervals: list[tuple[int, int]],
) -> list[int]:
    """Map lyric lines proportionally across active vocal intervals."""
    clean_intervals: list[tuple[int, int]] = []
    for start, end in intervals:
        s = max(0, int(start))
        e = max(s + 200, int(end))
        clean_intervals.append((s, e))

    total_active_ms = sum(e - s for s, e in clean_intervals)
    total_weight = sum(line_weights)
    if total_active_ms <= 0 or total_weight <= 0:
        return [idx * 3000 for idx in range(len(ref_lines))]

    cumulative_starts: list[float] = []
    running = 0
    for w in line_weights:
        cumulative_starts.append(running / total_weight)
        running += w

    starts_ms: list[int] = []
    for frac in cumulative_starts:
        target_active = frac * total_active_ms
        acc = 0.0
        mapped = clean_intervals[-1][1]
        for s, e in clean_intervals:
            seg_len = float(e - s)
            if acc + seg_len > target_active:
                mapped = int(round(s + (target_active - acc)))
                break
            acc += seg_len
        starts_ms.append(mapped)
    return starts_ms


def sync_lyrics_to_audio(
    existing_text: str,
    transcript: ListenTranscript,
    duration_ms: Optional[int] = None,
) -> list[TimedLine]:
    """Align existing lyric lines to timestamps from an audio transcript."""
    ref_lines = extract_clean_lines(existing_text)
    if not ref_lines:
        return []

    line_tokens = [tokenize_line(line) for line in ref_lines]
    line_weights = [max(1, len(toks)) for toks in line_tokens]

    flat_ref: list[_RefToken] = []
    for l_idx, toks in enumerate(line_tokens):
        for pos, tok in enumerate(toks):
            flat_ref.append(_RefToken(line_idx=l_idx, pos_in_line=pos, norm=tok))

    heard_pairs = _transcript_timed_words(transcript)
    aligned_pairs: list[tuple[int, int]] = []
    if flat_ref and heard_pairs:
        aligned_pairs = _align_token_sequences(
            [rt.norm for rt in flat_ref],
            [tok for _, tok in heard_pairs],
        )

    n_lines = len(ref_lines)
    anchors: list[Optional[int]] = [None] * n_lines

    if aligned_pairs:
        ms_per_word = _estimate_ms_per_word(heard_pairs)
        per_line_matches: dict[int, list[tuple[int, TimedWord]]] = {}
        for r_idx, h_idx in aligned_pairs:
            rt = flat_ref[r_idx]
            hw, _ = heard_pairs[h_idx]
            per_line_matches.setdefault(rt.line_idx, []).append((rt.pos_in_line, hw))

        for l_idx, matches in sorted(per_line_matches.items()):
            matches.sort(key=lambda item: item[0])
            chosen_pos, chosen_hw = matches[0]
            if len(matches) >= 2:
                second_pos, second_hw = matches[1]
                gap_limit = max(8000, (second_pos - chosen_pos) * 1500)
                if second_hw.start_ms - chosen_hw.start_ms > gap_limit:
                    chosen_pos, chosen_hw = second_pos, second_hw
            est = int(round(chosen_hw.start_ms - chosen_pos * ms_per_word))
            anchors[l_idx] = max(0, est)
    else:
        ms_per_word = 350.0

    anchored_indices = [i for i, val in enumerate(anchors) if val is not None]

    if not anchored_indices:
        # Fallback to vocal phrase segments (from energy backend) or track duration.
        if transcript.lines:
            intervals = [(ln.ms, ln.end_ms) for ln in transcript.lines]
        elif duration_ms and duration_ms > 2000:
            start_pad = min(3000, duration_ms // 10)
            end_pad = max(start_pad + 1000, duration_ms - 2000)
            intervals = [(start_pad, end_pad)]
        else:
            intervals = [(0, max(3000 * n_lines, 1000))]
        starts = _distribute_lines_across_intervals(ref_lines, line_weights, intervals)
    else:
        # Enforce strictly non-decreasing anchor timestamps before interpolation.
        prev_ms = -1
        for idx in anchored_indices:
            val = anchors[idx]
            assert val is not None
            if val < prev_ms:
                val = prev_ms + 200
            anchors[idx] = val
            prev_ms = val

        starts = [0] * n_lines
        for idx in anchored_indices:
            val = anchors[idx]
            assert val is not None
            starts[idx] = val

        # Leading unanchored lines before the first anchor.
        first_idx = anchored_indices[0]
        if first_idx > 0:
            t_first = starts[first_idx]
            needed = sum(line_weights[i] * ms_per_word + 250 for i in range(first_idx))
            start_floor = max(0.0, t_first - needed)
            span = max(0.0, t_first - start_floor)
            total_w = sum(line_weights[:first_idx])
            acc_w = 0
            for i in range(first_idx):
                starts[i] = int(round(start_floor + span * (acc_w / max(1, total_w))))
                acc_w += line_weights[i]

        # Intermediate gaps between anchored lines.
        for pos in range(len(anchored_indices) - 1):
            a = anchored_indices[pos]
            b = anchored_indices[pos + 1]
            if b <= a + 1:
                continue
            t_a = starts[a]
            t_b = max(t_a, starts[b])
            total_w = sum(line_weights[a:b])
            acc_w = line_weights[a]
            for i in range(a + 1, b):
                starts[i] = int(round(t_a + (t_b - t_a) * (acc_w / max(1, total_w))))
                acc_w += line_weights[i]

        # Trailing unanchored lines after the last anchor.
        last_idx = anchored_indices[-1]
        if last_idx < n_lines - 1:
            t_last = starts[last_idx]
            cur = float(t_last) + line_weights[last_idx] * ms_per_word + 250.0
            raw_trailing: list[float] = []
            for i in range(last_idx + 1, n_lines):
                raw_trailing.append(cur)
                cur += line_weights[i] * ms_per_word + 250.0

            if duration_ms and duration_ms > t_last + 1000 and raw_trailing[-1] > duration_ms - 500:
                end_cap = float(duration_ms - 500)
                span = max(0.0, end_cap - t_last)
                total_w = sum(line_weights[last_idx:n_lines])
                acc_w = line_weights[last_idx]
                for i in range(last_idx + 1, n_lines):
                    starts[i] = int(round(t_last + span * (acc_w / max(1, total_w))))
                    acc_w += line_weights[i]
            else:
                for offset, i in enumerate(range(last_idx + 1, n_lines)):
                    starts[i] = int(round(raw_trailing[offset]))

    # Final monotonic clamp so timestamps never go backwards.
    for i in range(1, n_lines):
        if starts[i] < starts[i - 1]:
            starts[i] = starts[i - 1]
    if duration_ms and duration_ms > 0:
        cap = max(0, duration_ms - 200)
        for i in range(n_lines):
            if starts[i] > cap:
                starts[i] = cap

    result: list[TimedLine] = []
    for i, text in enumerate(ref_lines):
        s_ms = max(0, int(starts[i]))
        if i + 1 < n_lines:
            e_ms = max(s_ms + 200, int(starts[i + 1]))
        else:
            est_end = s_ms + max(1000, line_weights[i] * 400)
            e_ms = min(duration_ms, est_end) if duration_ms and duration_ms > s_ms else est_end
        result.append(TimedLine(ms=s_ms, end_ms=e_ms, text=text))
    return result
