#!/usr/bin/env bash
set -euo pipefail

# Package verification deliberately inspects the archive Cargo produced. This
# catches a missing notice or embedded font even when the source tree has it.
package_target="${CARGO_TARGET_DIR:-target}"
package_listing="$(mktemp)"
package_output="$(mktemp)"
archive_listing="$(mktemp)"
package_extract="$(mktemp -d "${TMPDIR:-/tmp}/mathema-package.XXXXXX")"
trap 'rm -f "$package_listing" "$package_output" "$archive_listing"; rm -rf "$package_extract"' EXIT

read -r package_name package_version < <(
    cargo metadata --locked --no-deps --format-version 1 |
        python3 -c 'import json, sys; package = json.load(sys.stdin)["packages"][0]; print(package["name"], package["version"])'
)
package_prefix="$package_name-$package_version"

cargo package --locked --allow-dirty >"$package_output" 2>&1
if grep -E 'warning: ignoring (example|benchmark)' "$package_output" >/dev/null; then
    cat "$package_output" >&2
    echo "cargo package ignored a manifest-declared target" >&2
    exit 1
fi

cargo package --locked --allow-dirty --list >"$package_listing"
package_archive="$package_target/package/$package_prefix.crate"
if [[ ! -f "$package_archive" ]]; then
    echo "cargo package did not produce $package_archive" >&2
    exit 1
fi

tar -tzf "$package_archive" >"$archive_listing"

for path in \
    LICENSE \
    LICENSE-UNICODE \
    LICENSE-LATIN-MODERN.txt \
    LICENSE-LATIN-MODERN-MATH.txt \
    LICENSE-LPPL-1.3c.txt \
    MANIFEST-Latin-Modern.txt \
    MANIFEST-Latin-Modern-Math.txt \
    NOTICE-FONTS.txt; do
    grep -Fx "$path" "$package_listing" >/dev/null || {
        echo "package is missing $path" >&2
        exit 1
    }
    grep -Fx "$package_prefix/$path" "$archive_listing" >/dev/null || {
        echo "archive is missing $path" >&2
        exit 1
    }
done

font_paths=(
    src/render/fonts/LatinModernMath-Regular.otf
    src/render/fonts/lmmono10-italic.otf
    src/render/fonts/lmmono10-regular.otf
    src/render/fonts/lmmonolt10-bold.otf
    src/render/fonts/lmmonolt10-boldoblique.otf
    src/render/fonts/lmroman10-bold.otf
    src/render/fonts/lmroman10-bolditalic.otf
    src/render/fonts/lmroman10-italic.otf
    src/render/fonts/lmroman10-regular.otf
    src/render/fonts/lmsans10-bold.otf
    src/render/fonts/lmsans10-boldoblique.otf
    src/render/fonts/lmsans10-oblique.otf
    src/render/fonts/lmsans10-regular.otf
)
for font_path in "${font_paths[@]}"; do
    member="$package_prefix/$font_path"
    grep -Fx "$member" "$archive_listing" >/dev/null || {
        echo "archive is missing $font_path" >&2
        exit 1
    }
    expected_hash="$(tar -xOf "$package_archive" "$package_prefix/NOTICE-FONTS.txt" |
        awk -v path="$font_path" '$2 == path { print $1; exit }')"
    actual_hash="$(tar -xOf "$package_archive" "$member" | sha256sum | awk '{print $1}')"
    if [[ -z "$expected_hash" || "$actual_hash" != "$expected_hash" ]]; then
        echo "font hash mismatch for $font_path" >&2
        echo "expected: $expected_hash" >&2
        echo "actual:   $actual_hash" >&2
        exit 1
    fi
done

font_count="$(grep -E '\.otf$' "$archive_listing" | wc -l)"
if [[ "$font_count" -ne 13 ]]; then
    echo "package contains $font_count OTF files, expected 13" >&2
    exit 1
fi

tar -xzf "$package_archive" -C "$package_extract"
package_manifest="$package_extract/$package_prefix/Cargo.toml"
CARGO_TARGET_DIR="$package_target/package-target" \
    cargo check --locked --all-features --examples --benches --manifest-path "$package_manifest"

echo "verified $package_archive with $font_count embedded OTF files, required notices, and compilable examples/benches"
