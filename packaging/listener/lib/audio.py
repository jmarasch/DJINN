"""Audio loading, vocal isolation, Whisper transcription, and energy onset fallback."""

import importlib.util
import os
import re
import shutil
import subprocess
import sys
import tempfile
import wave
from pathlib import Path
from typing import Any, Optional

import numpy as np
from mutagen import File as MutagenFile

from .bootstrap import (
    DEFAULT_MODELS_DIR,
    configure_model_env,
    detect_nvidia_gpu,
    ensure_whisper_model,
)
from .models import ListenTranscript, TimedLine, TimedWord

SAMPLE_RATE = 16_000

_HALLUCINATION_PHRASES: tuple[str, ...] = (
    "thank you for watching",
    "thanks for watching",
    "thank you for listening",
    "thanks for listening",
    "please subscribe",
    "like and subscribe",
    "subtitles by",
    "captions by",
    "transcribed by",
    "translated by",
    "amara.org",
    "all rights reserved",
)

_BRACKETED_SOUND_RE = re.compile(r"^[\[(]\s*(?:music|applause|laughter|silence|instrumental|singing)\s*[\])]$", re.IGNORECASE)


def detect_available_backends() -> dict[str, bool]:
    """Check which audio tools and local ML packages are available."""
    return {
        "faster_whisper": importlib.util.find_spec("faster_whisper") is not None,
        "whisper": importlib.util.find_spec("whisper") is not None,
        "demucs": importlib.util.find_spec("demucs") is not None,
        "torch": importlib.util.find_spec("torch") is not None,
        "ctranslate2": importlib.util.find_spec("ctranslate2") is not None,
        "ffmpeg": shutil.which("ffmpeg") is not None,
    }


def probe_audio_duration_ms(audio_path: str) -> Optional[int]:
    """Read track duration in ms using mutagen, falling back to ffprobe."""
    if not audio_path or not os.path.isfile(audio_path):
        return None
    try:
        audio = MutagenFile(audio_path)
        info = getattr(audio, "info", None) if audio is not None else None
        length = getattr(info, "length", None)
        if isinstance(length, (int, float)) and length > 0:
            return int(round(length * 1000))
    except Exception:
        pass

    ffprobe = shutil.which("ffprobe")
    if ffprobe:
        try:
            proc = subprocess.run(
                [
                    ffprobe,
                    "-v",
                    "error",
                    "-show_entries",
                    "format=duration",
                    "-of",
                    "default=noprint_wrappers=1:nokey=1",
                    audio_path,
                ],
                capture_output=True,
                text=True,
                timeout=15,
                check=False,
            )
            if proc.returncode == 0 and proc.stdout.strip():
                sec = float(proc.stdout.strip())
                if sec > 0:
                    return int(round(sec * 1000))
        except Exception:
            pass
    return None


def _has_cuda(caps: dict[str, bool]) -> bool:
    if caps.get("ctranslate2"):
        try:
            import ctranslate2

            if ctranslate2.get_cuda_device_count() > 0:
                return True
        except Exception:
            pass
    if caps.get("torch"):
        try:
            import torch

            if torch.cuda.is_available():
                return True
        except Exception:
            pass
    return False


def _resolve_device(requested: str, caps: dict[str, bool]) -> str:
    if requested in ("cuda", "cpu"):
        return requested
    return "cuda" if _has_cuda(caps) else "cpu"


def resolve_compute_type(
    requested: str,
    device: str,
    model_name: str,
    gpu_info: Optional[tuple[str, int]],
) -> str:
    """Pick optimal CTranslate2 compute type for the active device and GPU VRAM."""
    if requested != "auto":
        return requested
    if device != "cuda":
        return "int8"
    lower_model = model_name.lower()
    if "large" in lower_model and (gpu_info is None or gpu_info[1] <= 10_000):
        return "int8_float16"
    return "float16"


def build_demucs_command(
    audio_path: str,
    demucs_out: Path,
    device: str,
    gpu_info: Optional[tuple[str, int]],
) -> list[str]:
    """Build the demucs.separate CLI command tuned for the GPU VRAM budget."""
    cmd = [
        sys.executable,
        "-m",
        "demucs.separate",
        "--two-stems=vocals",
        "-n",
        "htdemucs",
        "-d",
        device,
    ]
    if device == "cuda" and (gpu_info is None or gpu_info[1] <= 10_000):
        cmd.extend(["--segment", "7"])
    cmd.extend(["-o", str(demucs_out), str(audio_path)])
    return cmd


def _is_hallucination_line(text: str) -> bool:
    stripped = text.strip()
    if not stripped:
        return True
    if _BRACKETED_SOUND_RE.match(stripped):
        return True
    lower = stripped.lower()
    if any(phrase in lower for phrase in _HALLUCINATION_PHRASES):
        return True
    if lower in ("music", "you", "bye", ".", "...", ".."):
        return True
    return False


def _collapse_hallucinated_repeats(
    lines: list[TimedLine],
    words: list[TimedWord],
) -> tuple[list[TimedLine], list[TimedWord]]:
    """Trim pathological Whisper repetition loops (>8 identical consecutive lines)."""
    if len(lines) <= 8:
        return (lines, words)

    kept_lines: list[TimedLine] = []
    repeat_count = 0
    prev_norm = ""
    dropped_ranges: list[tuple[int, int]] = []

    for ln in lines:
        norm = " ".join(ln.text.lower().split())
        if norm and norm == prev_norm:
            repeat_count += 1
        else:
            repeat_count = 1
            prev_norm = norm

        if repeat_count > 8:
            dropped_ranges.append((ln.ms, ln.end_ms))
            continue
        kept_lines.append(ln)

    if not dropped_ranges:
        return (kept_lines, words)

    kept_words: list[TimedWord] = []
    for w in words:
        if any(start <= w.start_ms <= end for start, end in dropped_ranges):
            continue
        kept_words.append(w)
    return (kept_lines, kept_words)


def separate_vocals(
    audio_path: str,
    work_dir: Path,
    *,
    mode: str = "auto",
    device: str = "auto",
    use_ffmpeg_filter: bool = True,
    models_dir: Optional[Path] = None,
    gpu_info: Optional[tuple[str, int]] = None,
) -> tuple[str, bool]:
    """Isolate vocals via Demucs or ffmpeg vocal bandpass filter into `work_dir`."""
    caps = detect_available_backends()

    if mode == "none":
        return (audio_path, False)

    resolved_models = configure_model_env(models_dir or DEFAULT_MODELS_DIR)
    active_gpu = gpu_info if gpu_info is not None else detect_nvidia_gpu()

    use_demucs = mode == "demucs" or (mode == "auto" and caps["demucs"])
    if use_demucs:
        if not caps["demucs"]:
            raise RuntimeError(
                "Demucs vocal separation requested, but 'demucs' is not installed. "
                "Run with --bootstrap or pass --no-separate-vocals."
            )
        demucs_out = work_dir / "demucs"
        demucs_out.mkdir(parents=True, exist_ok=True)
        resolved_dev = _resolve_device(device, caps)
        cmd = build_demucs_command(str(audio_path), demucs_out, resolved_dev, active_gpu)
        env = os.environ.copy()
        env["TORCH_HOME"] = str(resolved_models / "demucs")
        env["HF_HOME"] = str(resolved_models / "hf")
        proc = subprocess.run(cmd, capture_output=True, text=True, env=env, check=False)
        if proc.returncode == 0:
            vocals_files = sorted(demucs_out.rglob("vocals.*"))
            if vocals_files:
                return (str(vocals_files[0]), True)
        if mode == "demucs":
            err_tail = (proc.stderr or proc.stdout or "").strip()[-400:]
            raise RuntimeError(f"Demucs failed to isolate vocals: {err_tail}")

    if mode in ("auto", "ffmpeg") and use_ffmpeg_filter:
        if not caps["ffmpeg"]:
            if mode == "ffmpeg":
                raise RuntimeError("ffmpeg not found on PATH for vocal filtering.")
            return (audio_path, False)
        filtered_wav = work_dir / "vocals_filtered.wav"
        cmd = [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(audio_path),
            "-vn",
            "-ac",
            "1",
            "-ar",
            str(SAMPLE_RATE),
            "-af",
            "pan=mono|c0=0.5*c0+0.5*c1,highpass=f=120,lowpass=f=7500,afftdn",
            str(filtered_wav),
        ]
        proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
        if proc.returncode == 0 and filtered_wav.is_file():
            return (str(filtered_wav), True)
        if mode == "ffmpeg":
            err_tail = (proc.stderr or "").strip()[-400:]
            raise RuntimeError(f"ffmpeg vocal filter failed: {err_tail}")

    return (audio_path, False)


def _load_mono_16k_float(audio_path: str) -> np.ndarray:
    """Decode an audio file into a 16 kHz mono float32 array in [-1, 1]."""
    ffmpeg = shutil.which("ffmpeg")
    if ffmpeg:
        proc = subprocess.run(
            [
                ffmpeg,
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                str(audio_path),
                "-vn",
                "-ac",
                "1",
                "-ar",
                str(SAMPLE_RATE),
                "-f",
                "s16le",
                "pipe:1",
            ],
            capture_output=True,
            check=False,
        )
        if proc.returncode == 0 and proc.stdout:
            pcm = np.frombuffer(proc.stdout, dtype=np.int16)
            return pcm.astype(np.float32) / 32768.0

    # Without ffmpeg (a DJINN node): faster-whisper's own decoder (PyAV).
    try:
        from faster_whisper.audio import decode_audio

        return decode_audio(str(audio_path), sampling_rate=SAMPLE_RATE)
    except ImportError:
        pass

    # Stdlib WAV fallback if ffmpeg is unavailable and file is PCM WAV.
    with wave.open(str(audio_path), "rb") as wf:
        channels = wf.getnchannels()
        sampwidth = wf.getsampwidth()
        rate = wf.getframerate()
        raw = wf.readframes(wf.getnframes())

    if sampwidth != 2:
        raise RuntimeError(
            f"Unsupported WAV sample width ({sampwidth} bytes) without ffmpeg."
        )
    samples = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
    if channels > 1:
        samples = samples.reshape(-1, channels).mean(axis=1)
    if rate != SAMPLE_RATE and len(samples) > 1:
        new_len = max(1, int(round(len(samples) * SAMPLE_RATE / rate)))
        old_idx = np.linspace(0.0, len(samples) - 1, num=len(samples))
        new_idx = np.linspace(0.0, len(samples) - 1, num=new_len)
        samples = np.interp(new_idx, old_idx, samples).astype(np.float32)
    return samples


def detect_vocal_intervals(
    samples: np.ndarray,
    *,
    sample_rate: int = SAMPLE_RATE,
    frame_ms: int = 20,
    merge_gap_ms: int = 350,
    min_segment_ms: int = 200,
    max_segment_ms: int = 6500,
) -> list[tuple[int, int]]:
    """Detect vocal-band energy phrase segments `(start_ms, end_ms)` from mono PCM."""
    frame_len = max(16, int(round(sample_rate * frame_ms / 1000.0)))
    n_frames = len(samples) // frame_len
    if n_frames <= 0:
        return []

    trimmed = samples[: n_frames * frame_len].reshape(n_frames, frame_len)
    window = np.hanning(frame_len).astype(np.float32)
    spec = np.fft.rfft(trimmed * window, axis=1)
    freqs = np.fft.rfftfreq(frame_len, d=1.0 / sample_rate)
    vocal_mask = (freqs >= 250.0) & (freqs <= 4000.0)

    if np.any(vocal_mask):
        band_power = np.mean(np.abs(spec[:, vocal_mask]) ** 2, axis=1)
        energy = np.sqrt(band_power)
    else:
        energy = np.sqrt(np.mean(trimmed ** 2, axis=1))

    peak = float(np.percentile(energy, 95))
    if peak <= 1e-3:
        return []

    noise_floor = float(np.percentile(energy, 15))
    threshold = min(peak * 0.35, max(1e-4, noise_floor * 2.0, peak * 0.12))
    active = energy >= threshold

    if not np.any(active):
        return []

    raw_segments: list[tuple[int, int]] = []
    start_idx: Optional[int] = None
    for idx, flag in enumerate(active):
        if flag and start_idx is None:
            start_idx = idx
        elif not flag and start_idx is not None:
            raw_segments.append((start_idx * frame_ms, idx * frame_ms))
            start_idx = None
    if start_idx is not None:
        raw_segments.append((start_idx * frame_ms, n_frames * frame_ms))

    # Merge short gaps inside vocal phrases.
    merged: list[tuple[int, int]] = []
    for s, e in raw_segments:
        if merged and s - merged[-1][1] <= merge_gap_ms:
            merged[-1] = (merged[-1][0], e)
        else:
            merged.append((s, e))

    # Drop tiny transient blips and split overly long phrases at local energy dips.
    final: list[tuple[int, int]] = []
    for s, e in merged:
        if e - s < min_segment_ms:
            continue
        if e - s <= max_segment_ms:
            final.append((s, e))
            continue

        cur_s = s
        while e - cur_s > max_segment_ms:
            search_lo = (cur_s + 2500) // frame_ms
            search_hi = min((e - 1000) // frame_ms, (cur_s + max_segment_ms) // frame_ms)
            if search_hi <= search_lo:
                split_ms = cur_s + max_segment_ms
            else:
                local_min = search_lo + int(np.argmin(energy[search_lo:search_hi]))
                split_ms = local_min * frame_ms
            if split_ms > cur_s + min_segment_ms:
                final.append((cur_s, split_ms))
            cur_s = split_ms
        if e - cur_s >= min_segment_ms:
            final.append((cur_s, e))

    return final


class ListenerEngine:
    """Reusable audio listener supporting faster-whisper, openai-whisper, and energy backends."""

    def __init__(
        self,
        *,
        backend: str = "auto",
        model: str = "medium",
        device: str = "auto",
        compute_type: str = "auto",
        language: Optional[str] = None,
        separate_vocals_mode: Optional[bool] = None,
        vocal_filter: bool = True,
        models_dir: Optional[Path] = None,
    ) -> None:
        self.requested_backend = backend
        self.model_name = model
        self.requested_device = device
        self.requested_compute_type = compute_type
        self.language = language
        self.separate_vocals_mode = separate_vocals_mode
        self.vocal_filter = vocal_filter
        self.models_dir = configure_model_env(models_dir or DEFAULT_MODELS_DIR)

        self._caps = detect_available_backends()
        self.gpu_info = detect_nvidia_gpu()
        self.resolved_backend = self._pick_backend()
        self.resolved_device = _resolve_device(device, self._caps)
        self.resolved_compute_type = resolve_compute_type(
            compute_type, self.resolved_device, self.model_name, self.gpu_info
        )
        self._fw_model: Any = None
        self._ow_model: Any = None

    def _pick_backend(self) -> str:
        if self.requested_backend == "auto":
            if self._caps["faster_whisper"]:
                return "faster-whisper"
            if self._caps["whisper"]:
                return "whisper"
            return "energy"

        if self.requested_backend == "faster-whisper" and not self._caps["faster_whisper"]:
            raise RuntimeError(
                "Backend 'faster-whisper' requested, but 'faster_whisper' is not installed. "
                "Run with --bootstrap to install dependencies or use --backend energy."
            )
        if self.requested_backend == "whisper" and not self._caps["whisper"]:
            raise RuntimeError(
                "Backend 'whisper' requested, but 'whisper' (openai-whisper) is not installed."
            )
        if self.requested_backend not in ("faster-whisper", "whisper", "energy"):
            raise ValueError(f"Unsupported backend: {self.requested_backend}")
        return self.requested_backend

    def _ensure_fw_model(self) -> Any:
        if self._fw_model is None:
            from faster_whisper import WhisperModel

            local_model_dir = ensure_whisper_model(self.model_name, self.models_dir)
            if self.resolved_device == "cuda" and self.gpu_info:
                print(
                    f"[audio] Loading faster-whisper '{self.model_name}' on CUDA GPU: "
                    f"{self.gpu_info[0]} ({self.gpu_info[1]} MiB, "
                    f"compute_type={self.resolved_compute_type}) from {local_model_dir}"
                )
            else:
                print(
                    f"[audio] Loading faster-whisper '{self.model_name}' on "
                    f"{self.resolved_device} (compute_type={self.resolved_compute_type}) "
                    f"from {local_model_dir}"
                )
            try:
                self._fw_model = WhisperModel(
                    str(local_model_dir),
                    device=self.resolved_device,
                    compute_type=self.resolved_compute_type,
                )
            except Exception as exc:
                if self.resolved_device == "cuda" or self.gpu_info is not None:
                    raise RuntimeError(
                        f"CUDA initialization failed for faster-whisper ({exc}). "
                        "Re-run with --bootstrap to verify CUDA runtime libraries "
                        "(nvidia-cublas-cu12, nvidia-cudnn-cu12) or pass --device cpu."
                    ) from exc
                raise
        return self._fw_model

    def _ensure_ow_model(self) -> Any:
        if self._ow_model is None:
            import whisper

            self._ow_model = whisper.load_model(
                self.model_name,
                device=self.resolved_device,
                download_root=str(self.models_dir / "whisper"),
            )
        return self._ow_model

    def _separation_mode(self) -> str:
        if self.separate_vocals_mode is True:
            return "demucs"
        if self.separate_vocals_mode is False:
            # Without ffmpeg (a DJINN node) the song is listened to as it is.
            return "ffmpeg" if self.vocal_filter and self._caps["ffmpeg"] else "none"
        # Auto mode: use Demucs for Whisper backends when installed, else ffmpeg filter.
        if self.resolved_backend in ("faster-whisper", "whisper") and self._caps["demucs"]:
            return "demucs"
        return "ffmpeg" if self.vocal_filter and self._caps["ffmpeg"] else "none"

    def transcribe(self, audio_path: str) -> ListenTranscript:
        """Separate vocals (if enabled) and transcribe or analyze `audio_path`."""
        if not os.path.isfile(audio_path):
            raise FileNotFoundError(f"Audio file not found: {audio_path}")

        sep_mode = self._separation_mode()
        with tempfile.TemporaryDirectory(prefix="djinn-listener-") as tmp_dir:
            vocal_path, vocal_separated = separate_vocals(
                audio_path,
                Path(tmp_dir),
                mode=sep_mode,
                device=self.resolved_device,
                use_ffmpeg_filter=self.vocal_filter,
                models_dir=self.models_dir,
                gpu_info=self.gpu_info,
            )

            if self.resolved_backend == "faster-whisper":
                return self._transcribe_faster_whisper(vocal_path, vocal_separated)
            if self.resolved_backend == "whisper":
                return self._transcribe_openai_whisper(vocal_path, vocal_separated)
            return self._analyze_energy(vocal_path, vocal_separated)

    def _transcribe_faster_whisper(
        self, vocal_path: str, vocal_separated: bool
    ) -> ListenTranscript:
        model = self._ensure_fw_model()
        segments, info = model.transcribe(
            vocal_path,
            beam_size=5,
            vad_filter=True,
            word_timestamps=True,
            condition_on_previous_text=False,
            no_speech_threshold=0.6,
            language=self.language or None,
        )

        lines: list[TimedLine] = []
        words: list[TimedWord] = []
        for seg in segments:
            seg_text = " ".join((getattr(seg, "text", "") or "").split())
            if _is_hallucination_line(seg_text):
                continue
            s_ms = max(0, int(round(float(seg.start) * 1000)))
            e_ms = max(s_ms, int(round(float(seg.end) * 1000)))
            lines.append(TimedLine(ms=s_ms, end_ms=e_ms, text=seg_text))

            seg_words = getattr(seg, "words", None) or []
            for w in seg_words:
                w_text = " ".join((getattr(w, "word", "") or "").split())
                if not w_text:
                    continue
                ws = max(0, int(round(float(w.start) * 1000)))
                we = max(ws, int(round(float(w.end) * 1000)))
                prob = float(getattr(w, "probability", 1.0) or 1.0)
                words.append(TimedWord(start_ms=ws, end_ms=we, word=w_text, probability=prob))

        lines, words = _collapse_hallucinated_repeats(lines, words)
        total_word_tokens = sum(len(ln.text.split()) for ln in lines)
        unique_tokens = {tok.lower() for ln in lines for tok in ln.text.split()}
        instrumental = total_word_tokens < 5 or (len(unique_tokens) < 3 and total_word_tokens < 6)
        detected_lang = getattr(info, "language", None) or self.language

        return ListenTranscript(
            lines=[] if instrumental else lines,
            words=[] if instrumental else words,
            instrumental=instrumental,
            model_name=f"faster-whisper/{self.model_name}",
            vocal_separated=vocal_separated,
            language=detected_lang,
        )

    def _transcribe_openai_whisper(
        self, vocal_path: str, vocal_separated: bool
    ) -> ListenTranscript:
        model = self._ensure_ow_model()
        result = model.transcribe(
            vocal_path,
            word_timestamps=True,
            condition_on_previous_text=False,
            language=self.language or None,
        )

        lines: list[TimedLine] = []
        words: list[TimedWord] = []
        for seg in result.get("segments", []) or []:
            seg_text = " ".join(str(seg.get("text", "")).split())
            if _is_hallucination_line(seg_text):
                continue
            s_ms = max(0, int(round(float(seg.get("start", 0.0)) * 1000)))
            e_ms = max(s_ms, int(round(float(seg.get("end", 0.0)) * 1000)))
            lines.append(TimedLine(ms=s_ms, end_ms=e_ms, text=seg_text))

            for w in seg.get("words", []) or []:
                w_text = " ".join(str(w.get("word", "")).split())
                if not w_text:
                    continue
                ws = max(0, int(round(float(w.get("start", 0.0)) * 1000)))
                we = max(ws, int(round(float(w.get("end", 0.0)) * 1000)))
                prob = float(w.get("probability", 1.0) or 1.0)
                words.append(TimedWord(start_ms=ws, end_ms=we, word=w_text, probability=prob))

        lines, words = _collapse_hallucinated_repeats(lines, words)
        total_word_tokens = sum(len(ln.text.split()) for ln in lines)
        unique_tokens = {tok.lower() for ln in lines for tok in ln.text.split()}
        instrumental = total_word_tokens < 5 or (len(unique_tokens) < 3 and total_word_tokens < 6)
        detected_lang = result.get("language") or self.language

        return ListenTranscript(
            lines=[] if instrumental else lines,
            words=[] if instrumental else words,
            instrumental=instrumental,
            model_name=f"whisper/{self.model_name}",
            vocal_separated=vocal_separated,
            language=detected_lang,
        )

    def _analyze_energy(
        self, vocal_path: str, vocal_separated: bool
    ) -> ListenTranscript:
        samples = _load_mono_16k_float(vocal_path)
        intervals = detect_vocal_intervals(samples)
        lines = [TimedLine(ms=s, end_ms=e, text="") for s, e in intervals]
        return ListenTranscript(
            lines=lines,
            words=[],
            instrumental=(len(lines) == 0),
            model_name="energy-onset",
            vocal_separated=vocal_separated,
            language=self.language,
        )
