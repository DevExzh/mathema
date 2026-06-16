use std::path::Path;

use mathema::Format;

/// Detect format from file extension.
pub fn detect_from_extension(path: &Path) -> Option<Format> {
    let ext = path.extension()?.to_str()?;
    match ext.to_ascii_lowercase().as_str() {
        "tex" | "latex" => Some(Format::LaTeX),
        "mml" | "mathml" => Some(Format::MathML),
        "omml" => Some(Format::Omml),
        "mtef" => Some(Format::Mtef),
        "um" | "unicodemath" => Some(Format::UnicodeMath),
        _ => None,
    }
}

/// Detect format by sniffing content.
pub fn detect_from_content(content: &str) -> Option<Format> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    // XML-based formats — check first because they're unambiguous
    if trimmed.starts_with('<') {
        if trimmed.contains("<m:oMath") || trimmed.contains("<m:r") {
            return Some(Format::Omml);
        }
        if trimmed.contains("<math")
            || trimmed.contains("<mi")
            || trimmed.contains("<mo")
            || trimmed.contains("<mn")
        {
            return Some(Format::MathML);
        }
    }

    // LaTeX — backslash commands are a strong signal
    if trimmed.starts_with('\\')
        || trimmed.contains("\\frac")
        || trimmed.contains("\\sqrt")
        || trimmed.contains("\\begin")
    {
        return Some(Format::LaTeX);
    }

    // MTEF — hex-encoded binary: even length, >= 8 chars, all hex digits
    if trimmed.len() >= 8
        && trimmed.len().is_multiple_of(2)
        && trimmed.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Some(Format::Mtef);
    }

    // UnicodeMath — presence of Unicode math operators
    if trimmed.chars().any(|c| {
        matches!(
            c,
            '\u{2200}'..='\u{22FF}' // Mathematical Operators block (includes summation, integral, sqrt, infinity)
            | '\u{2190}'..='\u{21FF}' // Arrows
            | '\u{2100}'..='\u{214F}' // Letterlike Symbols
        )
    }) {
        return Some(Format::UnicodeMath);
    }

    // Fallback: LaTeX-like ASCII with ^ or _ and braces
    if (trimmed.contains('^') || trimmed.contains('_'))
        && trimmed.contains('{')
        && trimmed.contains('}')
    {
        return Some(Format::LaTeX);
    }

    None
}

/// Detect format: try extension first, fall back to content sniffing.
pub fn detect_format(path: Option<&Path>, content: &str) -> Option<Format> {
    if let Some(p) = path
        && let Some(fmt) = detect_from_extension(p)
    {
        return Some(fmt);
    }
    detect_from_content(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // --- Extension-based detection ---

    #[test]
    fn ext_tex() {
        assert_eq!(
            detect_from_extension(Path::new("file.tex")),
            Some(Format::LaTeX)
        );
    }

    #[test]
    fn ext_latex() {
        assert_eq!(
            detect_from_extension(Path::new("file.latex")),
            Some(Format::LaTeX)
        );
    }

    #[test]
    fn ext_mml() {
        assert_eq!(
            detect_from_extension(Path::new("file.mml")),
            Some(Format::MathML)
        );
    }

    #[test]
    fn ext_mathml() {
        assert_eq!(
            detect_from_extension(Path::new("dir/file.mathml")),
            Some(Format::MathML)
        );
    }

    #[test]
    fn ext_omml() {
        assert_eq!(
            detect_from_extension(Path::new("file.omml")),
            Some(Format::Omml)
        );
    }

    #[test]
    fn ext_unknown() {
        assert_eq!(detect_from_extension(Path::new("file.txt")), None);
    }

    #[test]
    fn ext_no_extension() {
        assert_eq!(detect_from_extension(Path::new("file")), None);
    }

    // --- Content-based detection ---

    #[test]
    fn content_mathml() {
        assert_eq!(
            detect_from_content("<math><mi>x</mi></math>"),
            Some(Format::MathML)
        );
    }

    #[test]
    fn content_omml() {
        assert_eq!(
            detect_from_content("<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>"),
            Some(Format::Omml)
        );
    }

    #[test]
    fn content_latex_backslash() {
        assert_eq!(detect_from_content("\\frac{1}{2}"), Some(Format::LaTeX));
    }

    #[test]
    fn content_latex_braces() {
        assert_eq!(detect_from_content("x^{2}"), Some(Format::LaTeX));
    }

    #[test]
    fn content_mtef_hex() {
        assert_eq!(detect_from_content("0011AABBCCDD"), Some(Format::Mtef));
    }

    #[test]
    fn content_unicode_math() {
        assert_eq!(
            detect_from_content("\u{2211}_(i=0)^n i"),
            Some(Format::UnicodeMath)
        );
    }

    #[test]
    fn content_empty() {
        assert_eq!(detect_from_content(""), None);
    }

    // --- Combined detection ---

    #[test]
    fn combined_extension_wins() {
        assert_eq!(
            detect_format(Some(Path::new("file.tex")), "<math><mi>x</mi></math>"),
            Some(Format::LaTeX),
        );
    }

    #[test]
    fn combined_fallback_to_content() {
        assert_eq!(detect_format(None, "\\frac{1}{2}"), Some(Format::LaTeX),);
    }
}
