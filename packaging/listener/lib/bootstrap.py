"""Stdlib-only environment bootstrapper, CUDA path setup, and Hugging Face downloader."""

import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Optional

LISTENER_DIR = Path(__file__).resolve().parents[1]
DEFAULT_VENV_DIR = LISTENER_DIR / ".venv"
DEFAULT_MODELS_DIR = LISTENER_DIR / "models"
DEFAULT_REQUIREMENTS = LISTENER_DIR / "requirements.txt"

HF_WHISPER_REPOS: dict[str, str] = {
    "tiny": "Systran/faster-whisper-tiny",
    "tiny.en": "Systran/faster-whisper-tiny.en",
    "base": "Systran/faster-whisper-base",
    "base.en": "Systran/faster-whisper-base.en",
    "small": "Systran/faster-whisper-small",
    "small.en": "Systran/faster-whisper-small.en",
    "medium": "Systran/faster-whisper-medium",
    "medium.en": "Systran/faster-whisper-medium.en",
    "large": "Systran/faster-whisper-large-v3",
    "large-v2": "Systran/faster-whisper-large-v2",
    "large-v3": "Systran/faster-whisper-large-v3",
    "large-v3-turbo": "mobiuslabsgmbh/faster-whisper-large-v3-turbo",
    "turbo": "mobiuslabsgmbh/faster-whisper-large-v3-turbo",
}

_REQUIRED_IMPORTS: tuple[str, ...] = (
    "mutagen",
    "numpy",
    "huggingface_hub",
    "faster_whisper",
    "torch",
    "demucs",
)


def configure_model_env(models_dir: Optional[Path] = None) -> Path:
    """Create `models_dir` alongside listener.py and route HF/Torch caches into it."""
    target = Path(models_dir).expanduser().resolve() if models_dir else DEFAULT_MODELS_DIR
    hf_dir = target / "hf"
    demucs_dir = target / "demucs"
    whisper_dir = target / "whisper"

    target.mkdir(parents=True, exist_ok=True)
    hf_dir.mkdir(parents=True, exist_ok=True)
    demucs_dir.mkdir(parents=True, exist_ok=True)
    whisper_dir.mkdir(parents=True, exist_ok=True)

    os.environ["HF_HOME"] = str(hf_dir)
    os.environ["HF_HUB_CACHE"] = str(hf_dir / "hub")
    os.environ["TORCH_HOME"] = str(demucs_dir)
    return target


def select_venv_python() -> str:
    """Prefer Python 3.11/3.12/3.10 for prebuilt Linux CUDA binary wheels."""
    for candidate in ("python3.11", "python3.12", "python3.10"):
        found = shutil.which(candidate)
        if found:
            return found
    return sys.executable


def detect_nvidia_gpu() -> Optional[tuple[str, int]]:
    """Return `(gpu_name, vram_mib)` via nvidia-smi if an NVIDIA GPU is present."""
    nvidia_smi = shutil.which("nvidia-smi")
    if not nvidia_smi:
        return None
    try:
        proc = subprocess.run(
            [
                nvidia_smi,
                "--query-gpu=name,memory.total",
                "--format=csv,noheader,nounits",
            ],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except Exception:
        return None

    if proc.returncode != 0 or not proc.stdout.strip():
        return None

    first_line = proc.stdout.strip().splitlines()[0]
    parts = [p.strip() for p in first_line.split(",")]
    if len(parts) < 2:
        return None
    name = parts[0]
    try:
        vram_mib = int(float(parts[1]))
    except ValueError:
        return None
    return (name, vram_mib)


def venv_python_path(venv_dir: Path) -> Path:
    """Return the Python interpreter path inside `venv_dir`."""
    if os.name == "nt":
        return venv_dir / "Scripts" / "python.exe"
    return venv_dir / "bin" / "python"


def collect_nvidia_lib_dirs(venv_dir: Path) -> list[str]:
    """Find NVIDIA CUDA/cuDNN library directories inside `venv_dir` and system paths."""
    found: list[str] = []
    seen: set[str] = set()

    patterns = ("lib/python*/site-packages/nvidia/*/lib", "Lib/site-packages/nvidia/*/lib")
    for pat in patterns:
        for path in sorted(venv_dir.glob(pat)):
            if path.is_dir():
                resolved = str(path.resolve())
                if resolved not in seen:
                    seen.add(resolved)
                    found.append(resolved)

    sys_cuda_dir = Path("/usr/lib/x86_64-linux-gnu")
    if sys_cuda_dir.is_dir():
        resolved = str(sys_cuda_dir)
        if resolved not in seen:
            seen.add(resolved)
            found.append(resolved)

    return found


def prepend_ld_library_path(
    lib_dirs: list[str],
    env: Optional[dict[str, str]] = None,
) -> bool:
    """Prepend missing `lib_dirs` to LD_LIBRARY_PATH; True if venv dirs were missing."""
    if not lib_dirs:
        return False
    target_env = os.environ if env is None else env
    existing = [p for p in target_env.get("LD_LIBRARY_PATH", "").split(os.pathsep) if p]
    existing_set = set(existing)

    missing = [d for d in lib_dirs if d not in existing_set]
    if not missing:
        return False

    venv_missing = any("site-packages" in d for d in missing)
    target_env["LD_LIBRARY_PATH"] = os.pathsep.join(missing + existing)
    return venv_missing


def _requirements_digest(requirements_path: Path) -> str:
    if not requirements_path.is_file():
        return ""
    return hashlib.sha256(requirements_path.read_bytes()).hexdigest()


def _check_venv_imports(vpy: Path, modules: tuple[str, ...]) -> bool:
    if not vpy.is_file():
        return False
    code = "import " + ", ".join(modules)
    try:
        proc = subprocess.run(
            [str(vpy), "-c", code],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        return proc.returncode == 0
    except Exception:
        return False


def _is_inside_venv(venv_dir: Path, vpy: Path) -> bool:
    try:
        if Path(sys.prefix).resolve() == venv_dir.resolve():
            return True
        if Path(sys.executable).resolve() == vpy.resolve():
            return True
    except OSError:
        pass
    return False


def ensure_environment(
    venv_dir: Path = DEFAULT_VENV_DIR,
    requirements_path: Path = DEFAULT_REQUIREMENTS,
    *,
    force: bool = False,
) -> Path:
    """Create `venv_dir` and install dependencies (with CUDA libs when GPU present)."""
    venv_dir = Path(venv_dir).resolve()
    requirements_path = Path(requirements_path).resolve()
    vpy = venv_python_path(venv_dir)
    stamp_path = venv_dir / ".bootstrap-stamp.json"

    gpu_info = detect_nvidia_gpu()
    expected_stamp = {
        "requirements_sha256": _requirements_digest(requirements_path),
        "nvidia_gpu": gpu_info[0] if gpu_info else None,
        "platform": platform.system(),
        "machine": platform.machine(),
    }

    if not force and vpy.is_file() and stamp_path.is_file():
        try:
            current_stamp = json.loads(stamp_path.read_text(encoding="utf-8"))
        except Exception:
            current_stamp = None
        if current_stamp == expected_stamp and _check_venv_imports(vpy, _REQUIRED_IMPORTS):
            return vpy

    if not vpy.is_file():
        base_python = select_venv_python()
        print(f"[bootstrap] Creating virtual environment at {venv_dir} using {base_python}...")
        subprocess.run([base_python, "-m", "venv", str(venv_dir)], check=True)

    print("[bootstrap] Upgrading pip inside virtual environment...")
    subprocess.run([str(vpy), "-m", "pip", "install", "--upgrade", "pip"], check=True)

    if requirements_path.is_file():
        print(f"[bootstrap] Installing dependencies from {requirements_path}...")
        subprocess.run(
            [str(vpy), "-m", "pip", "install", "-r", str(requirements_path)],
            check=True,
        )

    if gpu_info is not None and platform.system() == "Linux" and platform.machine() == "x86_64":
        print(
            f"[bootstrap] Verifying CUDA runtime packages for {gpu_info[0]} ({gpu_info[1]} MiB)..."
        )
        subprocess.run(
            [
                str(vpy),
                "-m",
                "pip",
                "install",
                "nvidia-cublas-cu12",
                "nvidia-cudnn-cu12",
            ],
            check=True,
        )

    stamp_path.write_text(json.dumps(expected_stamp, indent=2) + "\n", encoding="utf-8")
    print(f"[bootstrap] Environment ready at {venv_dir}.")
    return vpy


def maybe_reexec_in_venv(
    venv_dir: Path,
    argv: list[str],
    *,
    requirements_path: Path = DEFAULT_REQUIREMENTS,
    force_bootstrap: bool = False,
) -> None:
    """Ensure `.venv` is provisioned and re-exec into it with CUDA library paths."""
    vpy = ensure_environment(
        venv_dir=venv_dir,
        requirements_path=requirements_path,
        force=force_bootstrap,
    )
    lib_dirs = collect_nvidia_lib_dirs(venv_dir)
    ld_needs_reexec = prepend_ld_library_path(lib_dirs)
    inside = _is_inside_venv(venv_dir, vpy)

    if (not inside or ld_needs_reexec) and os.environ.get("_LYRICS_LISTENER_REEXEC") != "1":
        os.environ["_LYRICS_LISTENER_REEXEC"] = "1"
        os.execv(str(vpy), [str(vpy), *argv])


def resolve_hf_whisper_repo(model_name: str) -> str:
    """Resolve a Whisper short name to its official Hugging Face CTranslate2 repo ID."""
    cleaned = model_name.strip()
    return HF_WHISPER_REPOS.get(cleaned, cleaned)


def _whisper_local_dir(model_name: str, models_dir: Path) -> Path:
    cleaned = model_name.strip()
    if cleaned in HF_WHISPER_REPOS:
        return models_dir / f"faster-whisper-{cleaned}"
    safe = cleaned.replace("/", "--").replace("\\", "--")
    if safe.startswith("faster-whisper-"):
        return models_dir / safe
    return models_dir / f"faster-whisper-{safe}"


PINNED_MODELS_FILE = Path(__file__).resolve().parents[1] / "models.json"


def pinned_model(model_name: str) -> Optional[dict]:
    """The pinned revision, sizes and checksums for a Whisper model, if it has them."""
    import json

    try:
        data = json.loads(PINNED_MODELS_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return data.get("models", {}).get(model_name.strip())


def verify_whisper_model(model_dir: Path, model_name: str, *, checksums: bool) -> list[str]:
    """Problems with a model folder against the pinned sizes (and, when asked,
    the model file's SHA-256); an empty list means it is the pinned model."""
    pin = pinned_model(model_name)
    if pin is None:
        return []
    problems: list[str] = []
    for name, spec in pin["files"].items():
        if name in (".gitattributes", "README.md"):
            continue
        path = model_dir / name
        if not path.is_file():
            problems.append(f"{name} is missing")
        elif path.stat().st_size != spec["size"]:
            problems.append(f"{name} has the wrong size")
        elif checksums and spec.get("sha256"):
            import hashlib

            digest = hashlib.sha256()
            with open(path, "rb") as fh:
                while chunk := fh.read(1 << 22):
                    digest.update(chunk)
            if digest.hexdigest() != spec["sha256"]:
                problems.append(f"{name} fails its checksum")
    return problems


def ensure_whisper_model(
    model_name: str,
    models_dir: Optional[Path] = None,
) -> Path:
    """Download a CTranslate2 Whisper model from Hugging Face into `models_dir`."""
    resolved_dir = configure_model_env(models_dir)

    local_candidate = Path(model_name)
    if local_candidate.is_dir() and (local_candidate / "model.bin").is_file():
        return local_candidate

    target_dir = _whisper_local_dir(model_name, resolved_dir)
    model_bin = target_dir / "model.bin"
    config_json = target_dir / "config.json"
    if model_bin.is_file() and config_json.is_file():
        for problem in verify_whisper_model(target_dir, model_name, checksums=False):
            print(f"[models] Warning: {model_name}: {problem}; it isn't the pinned model.")
        return target_dir

    repo_id = resolve_hf_whisper_repo(model_name)
    target_dir.mkdir(parents=True, exist_ok=True)
    print(f"[models] Downloading Whisper model '{repo_id}' into {target_dir}...")

    from huggingface_hub import snapshot_download

    pin = pinned_model(model_name)
    snapshot_download(
        repo_id=repo_id,
        revision=pin["revision"] if pin else None,
        local_dir=str(target_dir),
        cache_dir=str(resolved_dir / "hf"),
    )

    if not model_bin.is_file() or not config_json.is_file():
        raise RuntimeError(
            f"Downloaded model at {target_dir} is missing model.bin or config.json."
        )
    problems = verify_whisper_model(target_dir, model_name, checksums=True)
    if problems:
        raise RuntimeError(f"The {model_name} model didn't download correctly: {'; '.join(problems)}")
    return target_dir


def ensure_demucs_model(
    models_dir: Optional[Path] = None,
    model_name: str = "htdemucs",
) -> Path:
    """Pre-download Demucs stem separation weights into `models_dir/demucs`."""
    resolved_dir = configure_model_env(models_dir)
    demucs_dir = resolved_dir / "demucs"
    print(f"[models] Ensuring Demucs '{model_name}' model in {demucs_dir}...")
    from demucs.pretrained import get_model

    get_model(model_name)
    return demucs_dir
