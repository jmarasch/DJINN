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

Two kinds of job: "listen" makes the lyrics from the audio (they are
guesses, tagged AI-generated), "sync" times lyrics DJINN already has (tagged
AI-synced, with how well the audio matched the words).
"""

import argparse
import dataclasses
import json
import os
import signal
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from lib.aligner import compare_lyrics, sync_lyrics_to_audio  # noqa: E402
from lib.audio import ListenerEngine  # noqa: E402
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

    def next(self, count: int) -> dict:
        with self._request("POST", "/api/ai-work/next", {"max": count}) as res:
            return json.load(res)

    def audio(self, track_id: int, dest: Path) -> None:
        with self._request("GET", f"/api/ai-work/song/{track_id}", timeout=300) as res, open(dest, "wb") as out:
            while chunk := res.read(1 << 20):
                out.write(chunk)

    def result(self, track_id: int, lrc=None, error=None) -> None:
        with self._request("POST", f"/api/ai-work/result/{track_id}", {"lrc": lrc, "error": error}):
            pass


class Skip(Exception):
    """A song that can't be done, with why."""


def listen(engine: ListenerEngine, job: dict, audio: Path, min_similarity: float) -> str:
    """The `.lrc` text for one job."""
    query = SongQuery(
        artist=job.get("artist") or "",
        title=job.get("title") or "",
        album=job.get("album"),
        duration_ms=job.get("duration_ms"),
    )
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
            raise Skip(f"the audio doesn't match the words ({report.similarity * 100:.0f}%)")
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
        result = ListenerResult(tag_kind="ai-generated", model_name=transcript.model_name, synced_lrc=body)
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
    print(f"Listener ready: backend={engine.resolved_backend} model={engine.model_name} device={engine.resolved_device}", flush=True)
    while not STOP:
        try:
            batch = djinn.next(1)
        except (urllib.error.URLError, OSError, ValueError) as e:
            print(f"DJINN isn't answering ({e}); trying again soon", flush=True)
            time.sleep(30)
            continue
        jobs = batch.get("jobs") or []
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
                    audio = Path(tmp) / f"song.{EXTENSIONS.get(job.get('codec') or '', 'audio')}"
                    djinn.audio(job["track_id"], audio)
                    lrc = listen(engine, job, audio, args.min_similarity)
                djinn.result(job["track_id"], lrc=lrc)
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
    p.add_argument("--models-dir", default=str(DEFAULT_MODELS_DIR))
    p.add_argument("--min-similarity", type=float, default=0.5, help="how well the audio must match words to time them")
    p.add_argument("--once", action="store_true", help="do one batch (or wait once) and stop")
    p.add_argument("--setup", action="store_true", help="download the models and stop")
    args = p.parse_args()
    if args.setup:
        return setup(args)
    if not args.key:
        print("A worker key is needed (--key or DJINN_WORKER_KEY).", file=sys.stderr)
        return 2
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
