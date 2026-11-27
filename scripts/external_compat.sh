#!/usr/bin/env bash
set -euo pipefail

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'external compatibility check requires %s\n' "$1" >&2
        exit 2
    fi
}

require_command cargo
require_command pdflatex
require_command xmllint

mathml_xsd=${MATHEMA_MATHML_XSD:-}
omml_xsd=${MATHEMA_OMML_XSD:-}
xml_xsd=${MATHEMA_XML_XSD:-}
if [[ -z "$mathml_xsd" || -z "$omml_xsd" || -z "$xml_xsd" ]]; then
    cat >&2 <<'EOF'
set MATHEMA_MATHML_XSD to MathML 3's mathml3.xsd and MATHEMA_OMML_XSD to
ECMA-376's shared-math.xsd, and MATHEMA_XML_XSD to W3C's XML namespace
schema before running the external compatibility check.
EOF
    exit 2
fi
[[ -f "$mathml_xsd" ]] || { printf 'MathML schema does not exist: %s\n' "$mathml_xsd" >&2; exit 2; }
[[ -f "$omml_xsd" ]] || { printf 'OMML schema does not exist: %s\n' "$omml_xsd" >&2; exit 2; }
[[ -f "$xml_xsd" ]] || { printf 'XML namespace schema does not exist: %s\n' "$xml_xsd" >&2; exit 2; }

# The checked-in profile test uses Config::with_latex_profile with the public
# PdfLaTeX profile. It compiles ordinary, mixed, hexadecimal, nested, boxed,
# script, fraction, and colored expressions with the declared package set.
cargo test --quiet --locked --test latex_profile -- --nocapture

# The MathML profile test uses ConvertResult::to_mathml_with_options with the
# schema-oriented MathML 3 profile. The ordinary CLI route intentionally keeps
# private round-trip metadata, so validating it here would test the wrong
# public contract.
MATHEMA_MATHML_XSD="$mathml_xsd" \
    cargo test --quiet --locked --test mathml_profile \
    -- standard_profile_validates_with_external_mathml_schema --ignored --nocapture

# The checked-in OMML integration test emits and round-trips pure OMML,
# WordprocessingML host properties, and DrawingML host properties. Its ignored
# test uses the supplied ECMA schema directory and XML namespace schema. The
# Rust test retains its small fallback stubs for local runs without the full
# archive.
MATHEMA_XML_XSD="$xml_xsd" \
    cargo test --quiet --locked --test omml_round_trip

MATHEMA_OMML_XSD="$omml_xsd" \
MATHEMA_OMML_SCHEMA_DIR="$(dirname -- "$omml_xsd")" \
MATHEMA_XML_XSD="$xml_xsd" \
    cargo test --quiet --locked --test omml_round_trip \
    -- writer_output_validates_against_omml_xsd --ignored --nocapture

printf 'external compatibility checks passed\n'
