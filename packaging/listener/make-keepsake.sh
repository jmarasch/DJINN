#!/bin/sh
# Builds DJINN's AI keepsake: every Python package and model the lyrics
# listener needs, saved with checksums so it can be installed with no
# internet, whatever happens to PyPI, PyTorch's or Hugging Face's servers.
#
#   make-keepsake.sh OUTPUT_DIR [--models "tiny base small medium large-v3-turbo"]
#                    [--python python3] [--no-gpu] [--no-vocals]
#                    [--only packages|models]
#
# OUTPUT_DIR gets wheelhouse/{core,vocals-cpu,vocals-gpu,gpu}, models/ and
# SHA256SUMS. DJINN's Install button (Admin > AI) uses it when it is in
# <data folder>/ai-listener/keep, /opt/djinn/keep, or $DJINN_AI_KEEP. Build
# it with the same Python the target computer has (the wheels are per Python
# version), for example inside a container of the same Debian release.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out=${1:?usage: make-keepsake.sh OUTPUT_DIR [options]}
shift
models="tiny base small medium large-v3-turbo"
python=python3
gpu=1
vocals=1
only=all
while [ $# -gt 0 ]; do
    case "$1" in
        --models) models=$2; shift 2 ;;
        --python) python=$2; shift 2 ;;
        --no-gpu) gpu=0; shift ;;
        --no-vocals) vocals=0; shift ;;
        --only) only=$2; shift 2 ;;
        *) echo "unknown option $1" >&2; exit 2 ;;
    esac
done

mkdir -p "$out/wheelhouse" "$out/models"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
"$python" -m venv "$work/venv"
py="$work/venv/bin/python"
"$py" -m pip install -q --disable-pip-version-check --upgrade pip

get() { # name requirements [extra pip args]
    name=$1; req=$2; shift 2
    echo "Downloading $name packages"
    "$py" -m pip download -q --disable-pip-version-check -d "$out/wheelhouse/$name" -r "$here/$req" "$@"
}
if [ "$only" != models ]; then
    get core requirements.txt
    [ "$vocals" = 1 ] && get vocals-cpu requirements-vocals.txt --extra-index-url https://download.pytorch.org/whl/cpu
    [ "$vocals" = 1 ] && [ "$gpu" = 1 ] && get vocals-gpu requirements-vocals.txt
    [ "$gpu" = 1 ] && get gpu requirements-gpu.txt
fi
if [ "$only" = packages ]; then models=""; fi

# Install from the wheelhouse alone (proving it works with no internet),
# then let the worker's own setup fetch each model into the keepsake.
echo "Checking the packages install offline"
if [ "$only" = models ]; then
    # Only the models: the packages come from the internet, just to run the setup.
    "$py" -m pip install -q --disable-pip-version-check -r "$here/requirements.txt"
    [ "$vocals" = 1 ] && "$py" -m pip install -q --disable-pip-version-check -r "$here/requirements-vocals.txt" --extra-index-url https://download.pytorch.org/whl/cpu
else
    "$py" -m pip install -q --disable-pip-version-check --no-index --find-links "$out/wheelhouse/core" -r "$here/requirements.txt"
    if [ "$vocals" = 1 ]; then
        "$py" -m pip install -q --disable-pip-version-check --no-index --find-links "$out/wheelhouse/vocals-cpu" -r "$here/requirements-vocals.txt"
    fi
fi
first=1
for model in $models; do
    echo "Downloading the $model model"
    if [ "$vocals" = 1 ] && [ "$first" = 1 ]; then v=auto; else v=off; fi
    (cd "$here" && "$py" worker.py --setup --model "$model" --vocals "$v" --models-dir "$out/models")
    first=0
done

echo "Writing checksums"
(cd "$out" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
du -sh "$out"/wheelhouse/* "$out"/models/* 2>/dev/null
echo "Keepsake ready in $out"
