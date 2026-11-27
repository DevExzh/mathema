#!/usr/bin/env bash
set -euo pipefail

scratch=$(mktemp -d "${TMPDIR:-/tmp}/mathema-package.XXXXXX")
trap 'rm -rf -- "$scratch"' EXIT
if ! cargo package --locked >"$scratch/package.log" 2>&1; then
    cat "$scratch/package.log" >&2
    exit 1
fi
if grep -E 'warning: ignoring (example|benchmark)' "$scratch/package.log" >&2; then
    exit 1
fi
cargo metadata --locked --no-deps --format-version 1 >"$scratch/metadata.json"
python3 - "$scratch" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile

scratch = Path(sys.argv[1]).resolve()
meta = json.loads((scratch / "metadata.json").read_text())
manifest = Path("Cargo.toml").resolve()
package = next(p for p in meta["packages"] if Path(p["manifest_path"]) == manifest)
prefix = f"{package['name']}-{package['version']}"
target = Path(meta["target_directory"])
archive = target / "package" / (prefix + ".crate")
notices = (
    "LICENSE", "LICENSE-UNICODE", "LICENSE-LATIN-MODERN.txt",
    "LICENSE-LATIN-MODERN-MATH.txt", "LICENSE-LPPL-1.3c.txt",
    "MANIFEST-Latin-Modern.txt", "MANIFEST-Latin-Modern-Math.txt", "NOTICE-FONTS.txt",
)
with tarfile.open(archive) as crate:
    def contents(name):
        member = crate.extractfile(f"{prefix}/{name}")
        if member is None:
            sys.exit(f"Missing file in {archive}: {name}")
        return member.read()

    for path in notices:
        if contents(path) != Path(path).read_bytes():
            sys.exit(f"Packaged notice differs from source: {path}")
    sums = dict((path, digest) for digest, path in re.findall(
        r"^\s*([0-9a-f]{64})\s+(src/render/fonts/\S+\.otf)$",
        contents("NOTICE-FONTS.txt").decode(), re.MULTILINE))
    source_fonts = {p.as_posix() for p in Path("src/render/fonts").glob("*.otf")}
    archive_fonts = {p.removeprefix(prefix + "/") for p in crate.getnames() if p.endswith(".otf")}
    if not source_fonts or set(sums) != source_fonts or archive_fonts != source_fonts:
        sys.exit("Embedded fonts, archived fonts and NOTICE-FONTS.txt disagree")
    for path, expected in sums.items():
        if hashlib.sha256(contents(path)).hexdigest() != expected:
            sys.exit(f"Packaged font hash differs from NOTICE-FONTS.txt: {path}")

subprocess.run(["tar", "-xzf", archive, "-C", scratch], check=True)
subprocess.run(["cargo", "check", "--locked", "--all-features", "--examples", "--benches"],
               cwd=scratch / prefix, env={**os.environ, "CARGO_TARGET_DIR": str(target / "package-target")},
               check=True)
print(f"Verified {archive} with its notices, {len(sums)} fonts, examples and benchmarks")
PY
