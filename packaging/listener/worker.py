#!/usr/bin/env python3
"""DJINN's lyrics listener: a worker that makes or times lyrics with local AI.

It asks DJINN for songs that need lyrics (one at a time, so DJINN's rules
about when background work may run apply before every song), streams each
song from DJINN, listens to it with faster-whisper (and Demucs to isolate
the vocals when installed), and posts back a `.lrc` that DJINN stores with
an AI mark. Everything runs on this computer: the models are downloaded once
into ./models and nothing is sent anywhere but to your own DJINN.

    python worker.py --server http://127.0.0.1:8066 --key KEY      # run
    python worker.py --setup [--model medium]                      # download models
    python worker.py --bench [--model small]                       # speed test

Two kinds of job: "listen" makes the lyrics from the audio (they are
guesses, tagged AI-generated), "sync" times lyrics DJINN already has (tagged
AI-synced, with how well the audio matched the words).

Nodes help too: DJINN may ask this computer (when it has vocal isolation)
to "prepare" a song, isolating its vocals into a small 16 kHz FLAC for a
node, and a node's song may come as those vocals already ("vocals"), which
are listened to as they are.
"""

import argparse
import dataclasses
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib.aligner import compare_lyrics, sync_lyrics_to_audio  # noqa: E402
from lib.audio import ListenerEngine, detect_available_backends, separate_vocals  # noqa: E402
from lib.bootstrap import (  # noqa: E402
    DEFAULT_MODELS_DIR,
    configure_model_env,
    ensure_demucs_model,
    ensure_whisper_model,
)
from lib.lrc import format_lrc_file, lines_to_lrc_body  # noqa: E402
from lib.models import ListenerResult, SongQuery  # noqa: E402

EXTENSIONS = {"mpeg": "mp3", "mp4": "m4a", "flac": "flac", "vorbis": "ogg", "wav": "wav", "aac": "aac"}
MAX_WAIT = 600
STOP = False


def _stop(*_args):
    global STOP
    STOP = True


class Djinn:
    """The few calls the worker makes to DJINN."""

    def __init__(self, server: str, key: str):
        self.server = server.rstrip("/")
        self.key = key

    def _request(self, method: str, path: str, body=None, timeout: int = 60):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.server + path, data=data, method=method)
        req.add_header("Authorization", f"Worker {self.key}")
        if data is not None:
            req.add_header("Content-Type", "application/json")
        return urllib.request.urlopen(req, timeout=timeout)

    def next(self, count: int, can_isolate: bool, role: str = "all") -> dict:
        body = {"max": count, "can_isolate": can_isolate, "role": role}
        with self._request("POST", "/api/ai-work/next", body) as res:
            return json.load(res)

    def audio(self, track_id: int, dest: Path) -> None:
        with self._request("GET", f"/api/ai-work/song/{track_id}", timeout=300) as res, open(dest, "wb") as out:
            while chunk := res.read(1 << 20):
                out.write(chunk)

    def result(self, track_id: int, lrc=None, error=None, elapsed_ms=None) -> None:
        body = {"lrc": lrc, "error": error, "elapsed_ms": elapsed_ms}
        with self._request("POST", f"/api/ai-work/result/{track_id}", body):
            pass

    def vocals(self, track_id: int, flac: Path) -> None:
        req = urllib.request.Request(
            f"{self.server}/api/ai-work/vocals/{track_id}", data=flac.read_bytes(), method="POST"
        )
        req.add_header("Authorization", f"Worker {self.key}")
        req.add_header("Content-Type", "audio/flac")
        with urllib.request.urlopen(req, timeout=120):
            pass


class Skip(Exception):
    """A song that can't be done, with why."""


def prepare(engine: ListenerEngine, audio: Path, work: Path) -> Path:
    """Isolates a song's vocals into a small 16 kHz mono FLAC for a node."""
    vocals, separated = separate_vocals(
        str(audio),
        work,
        mode="demucs",
        device=engine.resolved_device,
        use_ffmpeg_filter=False,
        models_dir=engine.models_dir,
        gpu_info=engine.gpu_info,
    )
    if not separated:
        raise Skip("couldn't isolate the vocals")
    out = work / "vocals.flac"
    cmd = ["ffmpeg", "-y", "-hide_banner", "-loglevel", "error", "-i", vocals,
           "-vn", "-ac", "1", "-ar", "16000", "-sample_fmt", "s16", str(out)]
    proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if proc.returncode != 0 or not out.is_file():
        raise RuntimeError(f"ffmpeg couldn't write the vocals: {(proc.stderr or '').strip()[-200:]}")
    return out


def confidence(transcript) -> "float | None":
    """How sure the model was of the words it heard: the median of its word
    probabilities (0 to 1), or None when it gives none. The median, not the
    average: tried on real songs, clear vocals came out at 94 to 99% and
    mumbled or buried ones at 66 to 84%, where a few unsure words drag an
    average of good lyrics under 85%."""
    probs = sorted(w.probability for w in transcript.words if w.word.strip())
    if not probs:
        return None
    return probs[len(probs) // 2]


def too_low(score: float, min_similarity: float) -> Skip:
    # DJINN reads "scored too low" as final: asking again won't change it.
    return Skip(f"scored too low ({score * 100:.0f}%, under {min_similarity * 100:.0f}%)")


def listen(engine: ListenerEngine, job: dict, audio: Path, min_similarity: float) -> str:
    """The `.lrc` text for one job. `min_similarity` (0 to 1) is the least it
    may score: the match for timing, the model's confidence for words."""
    query = SongQuery(
        artist=job.get("artist") or "",
        title=job.get("title") or "",
        album=job.get("album"),
        duration_ms=job.get("duration_ms"),
    )
    if job.get("vocals"):
        # Isolated on Main already: listen to it as it is.
        saved = (engine.separate_vocals_mode, engine.vocal_filter)
        engine.separate_vocals_mode, engine.vocal_filter = False, False
        try:
            transcript = engine.transcribe(str(audio))
        finally:
            engine.separate_vocals_mode, engine.vocal_filter = saved
    else:
        transcript = engine.transcribe(str(audio))
    if transcript.instrumental:
        raise Skip("instrumental")
    plain = job.get("plain")
    if job["kind"] == "sync" and plain:
        report = compare_lyrics(
            plain,
            transcript,
            track_id=job["track_id"],
            artist=query.artist,
            title=query.title,
            existing_source="djinn",
            min_similarity=min_similarity,
        )
        is_energy = transcript.model_name == "energy-onset"
        if not is_energy and report.similarity < min_similarity:
            raise too_low(report.similarity, min_similarity)
        lines = sync_lyrics_to_audio(plain, transcript, duration_ms=query.duration_ms)
        if not lines:
            raise Skip("no lines to time")
        report = dataclasses.replace(report, synced_lines_created=len(lines))
        result = ListenerResult(
            tag_kind="ai-synced",
            model_name=transcript.model_name,
            synced_lrc=lines_to_lrc_body(lines),
            comparison=report,
        )
    else:
        body = lines_to_lrc_body(transcript.lines)
        if not body:
            raise Skip("no usable lyrics heard")
        sure = confidence(transcript)
        if sure is not None and sure < min_similarity:
            raise too_low(sure, min_similarity)
        result = ListenerResult(
            tag_kind="ai-generated", model_name=transcript.model_name, synced_lrc=body, confidence=sure
        )
    return format_lrc_file(query, result)


def run(args) -> int:
    if hasattr(os, "nice"):
        try:
            os.nice(19)  # background work: stay out of the way of everything else
        except OSError:
            pass
    signal.signal(signal.SIGTERM, _stop)
    signal.signal(signal.SIGINT, _stop)
    djinn = Djinn(args.server, args.key)
    engine = ListenerEngine(
        model=args.model,
        device=args.device,
        separate_vocals_mode=None if args.vocals == "auto" else args.vocals == "on",
        models_dir=Path(args.models_dir),
    )
    can_isolate = args.vocals != "off" and detect_available_backends().get("demucs", False)
    if args.role == "prepare":
        # Only separates vocals, so the listening model is never loaded.
        print(f"Vocal stripper ready: device={engine.resolved_device}", flush=True)
    else:
        print(
            f"Listener ready: backend={engine.resolved_backend} model={engine.model_name} "
            f"device={engine.resolved_device} isolates={can_isolate} role={args.role}",
            flush=True,
        )
    while not STOP:
        try:
            batch = djinn.next(1, can_isolate, args.role)
        except (urllib.error.URLError, OSError, ValueError) as e:
            print(f"DJINN isn't answering ({e}); trying again soon", flush=True)
            time.sleep(30)
            continue
        jobs = batch.get("jobs") or []
        # DJINN says the least a result may score (Admin > AI > Lyrics).
        if isinstance(batch.get("min_match"), (int, float)):
            min_similarity = max(0.0, min(1.0, batch["min_match"] / 100))
        else:
            min_similarity = args.min_similarity
        if not jobs:
            held = batch.get("held")
            wait = min(int(batch.get("wait_secs") or 120), MAX_WAIT)
            print(f"Nothing to do{' (' + json.dumps(held) + ')' if held else ''}; waiting {wait}s", flush=True)
            for _ in range(wait):
                if STOP:
                    break
                time.sleep(1)
            if args.once:
                return 0
            continue
        for job in jobs:
            started = time.time()
            try:
                with tempfile.TemporaryDirectory(prefix="djinn-listener-") as tmp:
                    ext = "flac" if job.get("vocals") else EXTENSIONS.get(job.get("codec") or "", "audio")
                    audio = Path(tmp) / f"song.{ext}"
                    djinn.audio(job["track_id"], audio)
                    if job.get("prepare"):
                        djinn.vocals(job["track_id"], prepare(engine, audio, Path(tmp)))
                        print(f"vocals ready: {job.get('artist')} - {job.get('title')} ({time.time() - started:.0f}s)", flush=True)
                        continue
                    listened = time.time()
                    lrc = listen(engine, job, audio, min_similarity)
                    elapsed = int((time.time() - listened) * 1000)
                djinn.result(job["track_id"], lrc=lrc, elapsed_ms=elapsed)
                print(f"{job['kind']}: {job.get('artist')} - {job.get('title')} ({time.time() - started:.0f}s)", flush=True)
            except Skip as why:
                djinn.result(job["track_id"], error=str(why))
                print(f"skipped {job.get('title')}: {why}", flush=True)
            except Exception as e:  # one song's trouble must not stop the worker
                print(f"failed {job.get('title')}: {e}", flush=True)
                try:
                    djinn.result(job["track_id"], error=str(e)[:200])
                except Exception:
                    pass
        if args.once:
            return 0
    return 0


def bench(args) -> int:
    """The speed test: how long the model takes to hear 30 seconds of sound,
    printed as JSON ({"encode_ms": N})."""
    import numpy as np
    from faster_whisper import WhisperModel

    models = configure_model_env(Path(args.models_dir))
    model_dir = ensure_whisper_model(args.model, models)
    device = "cpu" if args.device == "auto" else args.device
    model = WhisperModel(str(model_dir), device=device, compute_type="int8" if device == "cpu" else "float16")
    rng = np.random.default_rng(1)
    t = np.arange(16000 * 30) / 16000
    # A voice-like hum with noise, so the features look like sound.
    audio = (0.3 * np.sin(2 * np.pi * 180 * t) * (1 + np.sin(2 * np.pi * 3 * t)) + 0.05 * rng.standard_normal(t.size)).astype(np.float32)
    features = model.feature_extractor(audio)[:, :3000]
    model.encode(features)  # warm up
    tries = []
    for _ in range(2):
        start = time.time()
        model.encode(features)
        tries.append(time.time() - start)
    print(json.dumps({"encode_ms": int(min(tries) * 1000)}), flush=True)
    return 0


def setup(args) -> int:
    models = configure_model_env(Path(args.models_dir))
    print(f"Downloading the {args.model} model into {models} ...", flush=True)
    ensure_whisper_model(args.model, models)
    if args.vocals != "off":
        try:
            ensure_demucs_model(models)
        except Exception as e:  # vocal isolation is optional
            print(f"Vocal isolation model not available ({e}); continuing without it", flush=True)
    print("Models ready.", flush=True)
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--server", default="http://127.0.0.1:8066", help="DJINN's address")
    p.add_argument("--key", default=os.environ.get("DJINN_WORKER_KEY", ""), help="a worker key from Admin > AI")
    p.add_argument("--model", default="medium", help="whisper model: tiny, base, small, medium, large-v3-turbo")
    p.add_argument("--device", default="auto", help="auto, cuda or cpu")
    p.add_argument("--vocals", choices=["auto", "on", "off"], default="auto", help="isolate vocals with Demucs first")
    p.add_argument(
        "--role", choices=["all", "prepare", "listen"], default="all",
        help="all: whole songs; prepare: only separate vocals ahead; listen: take songs whose vocals are ready",
    )
    p.add_argument("--models-dir", default=str(DEFAULT_MODELS_DIR))
    p.add_argument(
        "--min-similarity", type=float, default=0.85,
        help="the least a result may score (0 to 1) when DJINN doesn't say: the match for timing, confidence for words",
    )
    p.add_argument("--once", action="store_true", help="do one batch (or wait once) and stop")
    p.add_argument("--setup", action="store_true", help="download the models and stop")
    p.add_argument("--bench", action="store_true", help="test this computer's speed with the model and stop")
    args = p.parse_args()
    if args.setup:
        return setup(args)
    if args.bench:
        return bench(args)
    if not args.key:
        print("A worker key is needed (--key or DJINN_WORKER_KEY).", file=sys.stderr)
        return 2
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
