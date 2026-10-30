"""Prints everything the GPU pack is built from, for a checkout.

The release workflow reuses the last release's GPU pack when this prints the
same for that release's checkout and for the commit being released, so the
slow CUDA build only runs when the pack could come out different:

- the djinn-ai crate's own files,
- every package the pack's build uses, with the features it's built with,
  as Cargo works them out for `-p djinn-ai --features cuda` on this system
  (test-only dependencies, and other crates' dependencies and features,
  don't count),
- the edition and build profiles from the root Cargo.toml,
- the GPU job in the release workflow, without its comments or the step
  that makes this comparison (changing how it compares doesn't change the
  pack).

DJINN's own version number is left out: the helper doesn't use it.

Usage: python .github/scripts/gpu-inputs.py <checkout directory>
Plain Python 3.8+ with no packages; needs git and cargo.
"""

import re
import subprocess
import sys
from pathlib import Path

CRATE = "djinn-ai"


def run(cwd, *args):
    return subprocess.run(args, cwd=cwd, check=True, capture_output=True, text=True).stdout


def packages(checkout):
    """The packages the pack's build uses, one line each with features.
    Workspace crates show without their version or folder."""
    tree = run(
        checkout,
        "cargo", "tree", "--locked", "--quiet",
        "-p", CRATE, "--features", "cuda",
        "-e", "normal,build",
        "--prefix", "none",
        "--format", "{p} [{f}]",
    )
    lines = set()
    for line in tree.splitlines():
        # "djinn-ai v0.2.5 (/path/to/crates/djinn-ai) [cuda]"
        local = re.match(r"^(\S+) v\S+ \((?:/|[A-Za-z]:\\)[^)]*\)(.*)$", line)
        if local:
            line = f"{local.group(1)} (workspace){local.group(2)}"
        if line.strip():
            lines.add(line)
    return sorted(lines)


def settings(checkout):
    """The edition and build profiles from the root Cargo.toml."""
    lines = []
    section = ""
    for line in (Path(checkout) / "Cargo.toml").read_text().splitlines():
        line = line.split("#", 1)[0].rstrip()
        header = re.match(r"^\[([^\]]+)\]$", line)
        if header:
            section = header.group(1)
            if section.startswith("profile"):
                lines.append(line)
        elif line and (
            section.startswith("profile")
            or (section == "workspace.package" and line.startswith("edition"))
        ):
            lines.append(line)
    return lines


def gpu_job(checkout):
    """The GPU job's part of the release workflow, without comments or the
    step that reuses the last pack."""
    workflow = (Path(checkout) / ".github/workflows/release.yml").read_text()
    match = re.search(r"^  gpu:\n.*?(?=^  [A-Za-z_-]+:\n)", workflow, re.M | re.S)
    job = match.group(0) if match else workflow
    job = re.sub(r"^      - name: Reuse the last release's pack\n.*?(?=^      - )", "", job, flags=re.M | re.S)
    return "\n".join(
        line.rstrip() for line in job.splitlines() if line.strip() and not line.lstrip().startswith("#")
    )


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    checkout = sys.argv[1]
    print(f"{CRATE} files", run(checkout, "git", "rev-parse", f"HEAD:crates/{CRATE}").strip())
    print("\n".join(packages(checkout)))
    print("\n".join(settings(checkout)))
    print(gpu_job(checkout))


if __name__ == "__main__":
    main()
