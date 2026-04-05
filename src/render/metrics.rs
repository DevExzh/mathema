/// Glyph metrics for mathematical typesetting.
/// All dimensions in em units (relative to font size).

#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    /// Height above baseline.
    pub height: f64,
    /// Depth below baseline.
    pub depth: f64,
    /// Advance width.
    pub width: f64,
    /// Italic correction.
    pub italic: f64,
}

impl GlyphMetrics {
    pub const fn new(height: f64, depth: f64, width: f64, italic: f64) -> Self {
        Self {
            height,
            depth,
            width,
            italic,
        }
    }

    /// Total vertical extent (height + depth).
    #[must_use]
    pub fn total_height(self) -> f64 {
        self.height + self.depth
    }
}

/// Look up glyph metrics for a character.
/// Falls back to heuristic defaults based on character category.
#[inline]
pub fn glyph_metrics(ch: char) -> GlyphMetrics {
    // Fast path: direct ASCII table lookup (O(1) vs O(log n) binary search)
    let code = ch as u32;
    if code < 128 {
        let m = &ASCII_METRICS[code as usize];
        // Sentinel: width == -1.0 means "not in table"
        if m.width >= 0.0 {
            return *m;
        }
        return heuristic_metrics(ch);
    }
    // Non-ASCII: binary search on sorted table
    if let Some(m) = lookup_metrics(ch) {
        return m;
    }
    heuristic_metrics(ch)
}

fn lookup_metrics(ch: char) -> Option<GlyphMetrics> {
    // Binary search on sorted table
    GLYPH_TABLE
        .binary_search_by_key(&ch, |&(c, _)| c)
        .ok()
        .map(|i| GLYPH_TABLE[i].1)
}

fn heuristic_metrics(ch: char) -> GlyphMetrics {
    if ch.is_ascii_uppercase() {
        GlyphMetrics::new(0.683, 0.0, 0.600, 0.0)
    } else if ch.is_ascii_lowercase() {
        if matches!(ch, 'b' | 'd' | 'f' | 'h' | 'k' | 'l' | 't') {
            GlyphMetrics::new(0.683, 0.0, 0.500, 0.0)
        } else if matches!(ch, 'g' | 'j' | 'p' | 'q' | 'y') {
            GlyphMetrics::new(0.431, 0.194, 0.500, 0.0)
        } else {
            GlyphMetrics::new(0.431, 0.0, 0.500, 0.0)
        }
    } else if ch.is_ascii_digit() {
        GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)
    } else if ch.is_ascii_punctuation() {
        GlyphMetrics::new(0.431, 0.0, 0.278, 0.0)
    } else {
        // Generic fallback for unknown characters
        GlyphMetrics::new(0.683, 0.0, 0.500, 0.0)
    }
}

// Sorted by Unicode codepoint for binary search.
// Metrics from Computer Modern / Latin Modern TFM files.
// Format: (char, GlyphMetrics { height, depth, width, italic })
static GLYPH_TABLE: &[(char, GlyphMetrics)] = &[
    // ASCII punctuation and operators (U+0021–U+003E)
    ('!', GlyphMetrics::new(0.694, 0.0, 0.278, 0.0)),
    ('(', GlyphMetrics::new(0.750, 0.250, 0.389, 0.0)),
    (')', GlyphMetrics::new(0.750, 0.250, 0.389, 0.0)),
    ('+', GlyphMetrics::new(0.583, 0.083, 0.778, 0.0)),
    (',', GlyphMetrics::new(0.106, 0.194, 0.278, 0.0)),
    ('-', GlyphMetrics::new(0.431, 0.0, 0.333, 0.0)),
    ('.', GlyphMetrics::new(0.106, 0.0, 0.278, 0.0)),
    ('/', GlyphMetrics::new(0.750, 0.250, 0.500, 0.0)),
    // ASCII digits (U+0030–U+0039)
    ('0', GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)),
    ('1', GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)),
    ('2', GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)),
    ('3', GlyphMetrics::new(0.644, 0.194, 0.500, 0.0)),
    ('4', GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)),
    ('5', GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)),
    ('6', GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)),
    ('7', GlyphMetrics::new(0.644, 0.194, 0.500, 0.0)),
    ('8', GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)),
    ('9', GlyphMetrics::new(0.644, 0.0, 0.500, 0.0)),
    // ASCII punctuation (U+003A–U+003E)
    (':', GlyphMetrics::new(0.431, 0.0, 0.278, 0.0)),
    (';', GlyphMetrics::new(0.431, 0.194, 0.278, 0.0)),
    ('<', GlyphMetrics::new(0.540, 0.040, 0.778, 0.0)),
    ('=', GlyphMetrics::new(0.390, 0.0, 0.778, 0.0)),
    ('>', GlyphMetrics::new(0.540, 0.040, 0.778, 0.0)),
    // Uppercase Latin (U+0041–U+005A, cmmi10 italic)
    ('A', GlyphMetrics::new(0.683, 0.0, 0.750, 0.0)),
    ('B', GlyphMetrics::new(0.683, 0.0, 0.759, 0.0)),
    ('C', GlyphMetrics::new(0.683, 0.0, 0.715, 0.0)),
    ('D', GlyphMetrics::new(0.683, 0.0, 0.828, 0.0)),
    ('E', GlyphMetrics::new(0.683, 0.0, 0.738, 0.0)),
    ('F', GlyphMetrics::new(0.683, 0.0, 0.643, 0.108)),
    ('G', GlyphMetrics::new(0.683, 0.0, 0.786, 0.0)),
    ('H', GlyphMetrics::new(0.683, 0.0, 0.831, 0.0)),
    ('I', GlyphMetrics::new(0.683, 0.0, 0.440, 0.0)),
    ('J', GlyphMetrics::new(0.683, 0.0, 0.555, 0.0)),
    ('K', GlyphMetrics::new(0.683, 0.0, 0.849, 0.0)),
    ('L', GlyphMetrics::new(0.683, 0.0, 0.681, 0.0)),
    ('M', GlyphMetrics::new(0.683, 0.0, 0.970, 0.0)),
    ('N', GlyphMetrics::new(0.683, 0.0, 0.803, 0.0)),
    ('O', GlyphMetrics::new(0.683, 0.0, 0.763, 0.0)),
    ('P', GlyphMetrics::new(0.683, 0.0, 0.642, 0.108)),
    ('Q', GlyphMetrics::new(0.683, 0.194, 0.791, 0.0)),
    ('R', GlyphMetrics::new(0.683, 0.0, 0.759, 0.0)),
    ('S', GlyphMetrics::new(0.683, 0.0, 0.613, 0.0)),
    ('T', GlyphMetrics::new(0.683, 0.0, 0.584, 0.108)),
    ('U', GlyphMetrics::new(0.683, 0.0, 0.683, 0.0)),
    ('V', GlyphMetrics::new(0.683, 0.0, 0.583, 0.0)),
    ('W', GlyphMetrics::new(0.683, 0.0, 0.944, 0.0)),
    ('X', GlyphMetrics::new(0.683, 0.0, 0.828, 0.0)),
    ('Y', GlyphMetrics::new(0.683, 0.0, 0.581, 0.108)),
    ('Z', GlyphMetrics::new(0.683, 0.0, 0.683, 0.0)),
    // Brackets (U+005B, U+005D)
    ('[', GlyphMetrics::new(0.750, 0.250, 0.278, 0.0)),
    (']', GlyphMetrics::new(0.750, 0.250, 0.278, 0.0)),
    // Lowercase Latin (U+0061–U+007A, cmmi10 italic)
    ('a', GlyphMetrics::new(0.431, 0.0, 0.529, 0.0)),
    ('b', GlyphMetrics::new(0.694, 0.0, 0.429, 0.0)),
    ('c', GlyphMetrics::new(0.431, 0.0, 0.433, 0.0)),
    ('d', GlyphMetrics::new(0.694, 0.0, 0.520, 0.0)),
    ('e', GlyphMetrics::new(0.431, 0.0, 0.466, 0.0)),
    ('f', GlyphMetrics::new(0.694, 0.194, 0.490, 0.108)),
    ('g', GlyphMetrics::new(0.431, 0.194, 0.477, 0.0)),
    ('h', GlyphMetrics::new(0.694, 0.0, 0.576, 0.0)),
    ('i', GlyphMetrics::new(0.668, 0.0, 0.345, 0.0)),
    ('j', GlyphMetrics::new(0.668, 0.194, 0.412, 0.0)),
    ('k', GlyphMetrics::new(0.694, 0.0, 0.521, 0.0)),
    ('l', GlyphMetrics::new(0.694, 0.0, 0.298, 0.0)),
    ('m', GlyphMetrics::new(0.431, 0.0, 0.878, 0.0)),
    ('n', GlyphMetrics::new(0.431, 0.0, 0.600, 0.0)),
    ('o', GlyphMetrics::new(0.431, 0.0, 0.485, 0.0)),
    ('p', GlyphMetrics::new(0.431, 0.194, 0.503, 0.0)),
    ('q', GlyphMetrics::new(0.431, 0.194, 0.446, 0.0)),
    ('r', GlyphMetrics::new(0.431, 0.0, 0.451, 0.0)),
    ('s', GlyphMetrics::new(0.431, 0.0, 0.469, 0.0)),
    ('t', GlyphMetrics::new(0.615, 0.0, 0.361, 0.0)),
    ('u', GlyphMetrics::new(0.431, 0.0, 0.572, 0.0)),
    ('v', GlyphMetrics::new(0.431, 0.0, 0.485, 0.0)),
    ('w', GlyphMetrics::new(0.431, 0.0, 0.716, 0.0)),
    ('x', GlyphMetrics::new(0.431, 0.0, 0.572, 0.0)),
    ('y', GlyphMetrics::new(0.431, 0.194, 0.490, 0.0)),
    ('z', GlyphMetrics::new(0.431, 0.0, 0.465, 0.0)),
    // Braces and pipe (U+007B–U+007D)
    ('{', GlyphMetrics::new(0.750, 0.250, 0.500, 0.0)),
    ('|', GlyphMetrics::new(0.750, 0.250, 0.278, 0.0)),
    ('}', GlyphMetrics::new(0.750, 0.250, 0.500, 0.0)),
    // Latin-1 supplement (U+00B1, U+00D7, U+00F7)
    ('\u{00B1}', GlyphMetrics::new(0.583, 0.083, 0.778, 0.0)), // ± plus-minus
    ('\u{00D7}', GlyphMetrics::new(0.491, 0.0, 0.778, 0.0)),   // × times
    ('\u{00F7}', GlyphMetrics::new(0.583, 0.083, 0.778, 0.0)), // ÷ divide
    // Greek uppercase (U+0393–U+03A9)
    ('\u{0393}', GlyphMetrics::new(0.683, 0.0, 0.625, 0.0)), // Γ Gamma
    ('\u{0394}', GlyphMetrics::new(0.683, 0.0, 0.833, 0.0)), // Δ Delta
    ('\u{0398}', GlyphMetrics::new(0.683, 0.0, 0.778, 0.0)), // Θ Theta
    ('\u{039B}', GlyphMetrics::new(0.683, 0.0, 0.694, 0.0)), // Λ Lambda
    ('\u{039E}', GlyphMetrics::new(0.683, 0.0, 0.667, 0.0)), // Ξ Xi
    ('\u{03A0}', GlyphMetrics::new(0.683, 0.0, 0.750, 0.0)), // Π Pi
    ('\u{03A3}', GlyphMetrics::new(0.683, 0.0, 0.722, 0.0)), // Σ Sigma
    ('\u{03A5}', GlyphMetrics::new(0.683, 0.0, 0.778, 0.0)), // Υ Upsilon
    ('\u{03A6}', GlyphMetrics::new(0.683, 0.0, 0.722, 0.0)), // Φ Phi
    ('\u{03A8}', GlyphMetrics::new(0.683, 0.0, 0.778, 0.0)), // Ψ Psi
    ('\u{03A9}', GlyphMetrics::new(0.683, 0.0, 0.722, 0.0)), // Ω Omega
    // Greek lowercase (U+03B1–U+03C9)
    ('\u{03B1}', GlyphMetrics::new(0.431, 0.0, 0.564, 0.0)), // α alpha
    ('\u{03B2}', GlyphMetrics::new(0.694, 0.194, 0.517, 0.0)), // β beta
    ('\u{03B3}', GlyphMetrics::new(0.431, 0.194, 0.500, 0.0)), // γ gamma
    ('\u{03B4}', GlyphMetrics::new(0.694, 0.0, 0.451, 0.0)), // δ delta
    ('\u{03B5}', GlyphMetrics::new(0.431, 0.0, 0.438, 0.0)), // ε epsilon
    ('\u{03B6}', GlyphMetrics::new(0.694, 0.194, 0.438, 0.0)), // ζ zeta
    ('\u{03B7}', GlyphMetrics::new(0.431, 0.194, 0.490, 0.0)), // η eta
    ('\u{03B8}', GlyphMetrics::new(0.694, 0.0, 0.500, 0.0)), // θ theta
    ('\u{03B9}', GlyphMetrics::new(0.431, 0.0, 0.354, 0.0)), // ι iota
    ('\u{03BA}', GlyphMetrics::new(0.431, 0.0, 0.521, 0.0)), // κ kappa
    ('\u{03BB}', GlyphMetrics::new(0.694, 0.0, 0.557, 0.0)), // λ lambda
    ('\u{03BC}', GlyphMetrics::new(0.431, 0.194, 0.576, 0.0)), // μ mu
    ('\u{03BD}', GlyphMetrics::new(0.431, 0.0, 0.450, 0.0)), // ν nu
    ('\u{03BE}', GlyphMetrics::new(0.694, 0.194, 0.438, 0.0)), // ξ xi
    ('\u{03C0}', GlyphMetrics::new(0.431, 0.0, 0.550, 0.0)), // π pi
    ('\u{03C1}', GlyphMetrics::new(0.431, 0.194, 0.500, 0.0)), // ρ rho
    ('\u{03C2}', GlyphMetrics::new(0.431, 0.194, 0.439, 0.0)), // ς final sigma
    ('\u{03C3}', GlyphMetrics::new(0.431, 0.0, 0.536, 0.0)), // σ sigma
    ('\u{03C4}', GlyphMetrics::new(0.431, 0.0, 0.443, 0.0)), // τ tau
    ('\u{03C5}', GlyphMetrics::new(0.431, 0.0, 0.500, 0.0)), // υ upsilon
    ('\u{03C6}', GlyphMetrics::new(0.431, 0.194, 0.557, 0.0)), // φ phi
    ('\u{03C7}', GlyphMetrics::new(0.431, 0.194, 0.517, 0.0)), // χ chi
    ('\u{03C8}', GlyphMetrics::new(0.694, 0.194, 0.590, 0.0)), // ψ psi
    ('\u{03C9}', GlyphMetrics::new(0.431, 0.0, 0.600, 0.0)), // ω omega
    // General punctuation (U+2026)
    ('\u{2026}', GlyphMetrics::new(0.106, 0.0, 1.000, 0.0)), // … ldots
    // Arrows (U+2190–U+21D4)
    ('\u{2190}', GlyphMetrics::new(0.511, 0.011, 1.000, 0.0)), // ← leftarrow
    ('\u{2191}', GlyphMetrics::new(0.694, 0.194, 0.500, 0.0)), // ↑ uparrow
    ('\u{2192}', GlyphMetrics::new(0.511, 0.011, 1.000, 0.0)), // → rightarrow
    ('\u{2193}', GlyphMetrics::new(0.694, 0.194, 0.500, 0.0)), // ↓ downarrow
    ('\u{2194}', GlyphMetrics::new(0.511, 0.011, 1.000, 0.0)), // ↔ leftrightarrow
    ('\u{21D0}', GlyphMetrics::new(0.511, 0.011, 1.000, 0.0)), // ⇐ Leftarrow
    ('\u{21D2}', GlyphMetrics::new(0.511, 0.011, 1.000, 0.0)), // ⇒ Rightarrow
    ('\u{21D4}', GlyphMetrics::new(0.511, 0.011, 1.000, 0.0)), // ⇔ Leftrightarrow
    // Mathematical operators (U+2200–U+222E)
    ('\u{2200}', GlyphMetrics::new(0.694, 0.0, 0.556, 0.0)), // ∀ forall
    ('\u{2202}', GlyphMetrics::new(0.694, 0.0, 0.500, 0.0)), // ∂ partial
    ('\u{2203}', GlyphMetrics::new(0.694, 0.0, 0.556, 0.0)), // ∃ exists
    ('\u{2205}', GlyphMetrics::new(0.772, 0.078, 0.500, 0.0)), // ∅ emptyset
    ('\u{2207}', GlyphMetrics::new(0.683, 0.0, 0.833, 0.0)), // ∇ nabla
    ('\u{2208}', GlyphMetrics::new(0.540, 0.040, 0.667, 0.0)), // ∈ in
    ('\u{2209}', GlyphMetrics::new(0.750, 0.250, 0.667, 0.0)), // ∉ notin
    ('\u{220B}', GlyphMetrics::new(0.540, 0.040, 0.667, 0.0)), // ∋ ni
    ('\u{220F}', GlyphMetrics::new(0.750, 0.250, 1.000, 0.0)), // ∏ product
    ('\u{2211}', GlyphMetrics::new(0.750, 0.250, 1.000, 0.0)), // ∑ sum
    ('\u{221A}', GlyphMetrics::new(0.800, 0.200, 0.833, 0.0)), // √ sqrt
    ('\u{221E}', GlyphMetrics::new(0.431, 0.0, 0.778, 0.0)), // ∞ infty
    ('\u{2227}', GlyphMetrics::new(0.598, 0.0, 0.667, 0.0)), // ∧ wedge
    ('\u{2228}', GlyphMetrics::new(0.598, 0.0, 0.667, 0.0)), // ∨ vee
    ('\u{2229}', GlyphMetrics::new(0.598, 0.0, 0.667, 0.0)), // ∩ cap
    ('\u{222A}', GlyphMetrics::new(0.598, 0.0, 0.667, 0.0)), // ∪ cup
    ('\u{222B}', GlyphMetrics::new(0.750, 0.250, 0.556, 0.144)), // ∫ integral
    ('\u{222C}', GlyphMetrics::new(0.750, 0.250, 0.944, 0.144)), // ∬ double integral
    ('\u{222D}', GlyphMetrics::new(0.750, 0.250, 1.333, 0.144)), // ∭ triple integral
    ('\u{222E}', GlyphMetrics::new(0.750, 0.250, 0.556, 0.144)), // ∮ contour integral
    // Relations (U+2260–U+2297)
    ('\u{2260}', GlyphMetrics::new(0.694, 0.194, 0.778, 0.0)), // ≠ neq
    ('\u{2264}', GlyphMetrics::new(0.636, 0.136, 0.778, 0.0)), // ≤ leq
    ('\u{2265}', GlyphMetrics::new(0.636, 0.136, 0.778, 0.0)), // ≥ geq
    ('\u{2282}', GlyphMetrics::new(0.540, 0.040, 0.778, 0.0)), // ⊂ subset
    ('\u{2283}', GlyphMetrics::new(0.540, 0.040, 0.778, 0.0)), // ⊃ supset
    ('\u{2286}', GlyphMetrics::new(0.636, 0.136, 0.778, 0.0)), // ⊆ subseteq
    ('\u{2287}', GlyphMetrics::new(0.636, 0.136, 0.778, 0.0)), // ⊇ supseteq
    ('\u{2295}', GlyphMetrics::new(0.583, 0.083, 0.778, 0.0)), // ⊕ oplus
    ('\u{2297}', GlyphMetrics::new(0.583, 0.083, 0.778, 0.0)), // ⊗ otimes
    // Large n-ary operators and dots (U+22C0–U+22F1)
    ('\u{22C0}', GlyphMetrics::new(0.750, 0.250, 0.833, 0.0)), // ⋀ bigwedge
    ('\u{22C1}', GlyphMetrics::new(0.750, 0.250, 0.833, 0.0)), // ⋁ bigvee
    ('\u{22C2}', GlyphMetrics::new(0.750, 0.250, 0.833, 0.0)), // ⋂ bigcap
    ('\u{22C3}', GlyphMetrics::new(0.750, 0.250, 0.833, 0.0)), // ⋃ bigcup
    ('\u{22EE}', GlyphMetrics::new(0.900, 0.0, 0.278, 0.0)),   // ⋮ vdots
    ('\u{22EF}', GlyphMetrics::new(0.310, 0.0, 1.000, 0.0)),   // ⋯ cdots
    ('\u{22F1}', GlyphMetrics::new(0.900, 0.0, 1.000, 0.0)),   // ⋱ ddots
    // Delimiters (U+2308–U+230B)
    ('\u{2308}', GlyphMetrics::new(0.750, 0.250, 0.389, 0.0)), // ⌈ lceil
    ('\u{2309}', GlyphMetrics::new(0.750, 0.250, 0.389, 0.0)), // ⌉ rceil
    ('\u{230A}', GlyphMetrics::new(0.750, 0.250, 0.389, 0.0)), // ⌊ lfloor
    ('\u{230B}', GlyphMetrics::new(0.750, 0.250, 0.389, 0.0)), // ⌋ rfloor
    // Angle brackets (U+27E8–U+27E9)
    ('\u{27E8}', GlyphMetrics::new(0.750, 0.250, 0.389, 0.0)), // ⟨ langle
    ('\u{27E9}', GlyphMetrics::new(0.750, 0.250, 0.389, 0.0)), // ⟩ rangle
    // Large operators (U+2A00–U+2A02)
    ('\u{2A00}', GlyphMetrics::new(0.750, 0.250, 1.000, 0.0)), // ⨀ bigodot
    ('\u{2A01}', GlyphMetrics::new(0.750, 0.250, 1.000, 0.0)), // ⨁ bigoplus
    ('\u{2A02}', GlyphMetrics::new(0.750, 0.250, 1.000, 0.0)), // ⨂ bigotimes
];

/// Sentinel value: width == -1.0 means "no entry in table; fall through to heuristic".
const NO_ENTRY: GlyphMetrics = GlyphMetrics::new(0.0, 0.0, -1.0, 0.0);

/// Direct lookup table for ASCII characters (0..128). Built at compile time from GLYPH_TABLE.
/// Entries not in GLYPH_TABLE use the NO_ENTRY sentinel.
static ASCII_METRICS: [GlyphMetrics; 128] = build_ascii_table();

const fn build_ascii_table() -> [GlyphMetrics; 128] {
    let mut table = [NO_ENTRY; 128];
    let mut i = 0;
    while i < GLYPH_TABLE.len() {
        let (ch, m) = GLYPH_TABLE[i];
        let code = ch as u32;
        if code < 128 {
            table[code as usize] = m;
        }
        i += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_glyph_lookup() {
        let m = glyph_metrics('A');
        assert!((m.height - 0.683).abs() < 0.001);
        assert!((m.width - 0.750).abs() < 0.001);
    }

    #[test]
    fn digit_lookup() {
        let m = glyph_metrics('5');
        assert!((m.height - 0.644).abs() < 0.001);
    }

    #[test]
    fn greek_lookup() {
        let m = glyph_metrics('\u{03B1}'); // alpha
        assert!((m.height - 0.431).abs() < 0.001);
    }

    #[test]
    fn sum_lookup() {
        let m = glyph_metrics('\u{2211}'); // ∑
        assert!((m.height - 0.750).abs() < 0.001);
        assert!((m.depth - 0.250).abs() < 0.001);
    }

    #[test]
    fn fallback_for_unknown() {
        let m = glyph_metrics('\u{1F600}'); // emoji - unknown
        assert!(m.height > 0.0);
        assert!(m.width > 0.0);
    }

    #[test]
    fn table_is_sorted() {
        for window in GLYPH_TABLE.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "GLYPH_TABLE not sorted: {:?} >= {:?}",
                window[0].0,
                window[1].0
            );
        }
    }

    #[test]
    fn uppercase_latin_lookup() {
        let m = glyph_metrics('A');
        assert!(
            (m.height - 0.683).abs() < 0.001,
            "A height should be ~0.683"
        );
        assert!(m.width > 0.0);
    }

    #[test]
    fn lowercase_latin_lookup() {
        let m = glyph_metrics('x');
        assert!(
            (m.height - 0.431).abs() < 0.001,
            "x height should be ~0.431 (x-height)"
        );
        assert!(m.depth.abs() < 0.001, "x should have no depth");
    }

    #[test]
    fn descender_char_has_depth() {
        let m = glyph_metrics('y');
        assert!(m.depth > 0.0, "y should have depth (descender)");
    }

    #[test]
    fn operator_plus_lookup() {
        let m = glyph_metrics('+');
        assert!(m.height > 0.0);
        assert!(m.width > 0.0);
    }

    #[test]
    fn parenthesis_lookup() {
        let m = glyph_metrics('(');
        assert!(m.height > 0.0);
        assert!(m.depth > 0.0, "parenthesis should have depth");
    }

    #[test]
    fn integral_sign_lookup() {
        let m = glyph_metrics('\u{222B}'); // ∫
        assert!(m.height > 0.0);
        assert!(m.depth > 0.0, "integral sign should have depth");
    }

    #[test]
    fn pi_greek_lookup() {
        let m = glyph_metrics('\u{03C0}'); // π
        assert!(m.height > 0.0);
        assert!(m.width > 0.0);
    }

    #[test]
    fn capital_greek_lookup() {
        let m = glyph_metrics('\u{0393}'); // Γ
        assert!(
            (m.height - 0.683).abs() < 0.001,
            "capital Greek should match Latin capitals"
        );
    }

    #[test]
    fn equals_sign_lookup() {
        let m = glyph_metrics('=');
        assert!(m.height > 0.0);
        assert!(m.width > 0.0);
    }

    #[test]
    fn fallback_cjk_character() {
        let m = glyph_metrics('\u{4E00}'); // 一 (CJK)
        assert!(m.height > 0.0, "CJK fallback should provide height");
        assert!(m.width > 0.0, "CJK fallback should provide width");
    }

    #[test]
    fn fallback_for_control_char() {
        let m = glyph_metrics('\u{0001}'); // control char
        assert!(
            m.width >= 0.0,
            "control char should have non-negative width"
        );
    }

    #[test]
    fn bracket_lookup() {
        let m = glyph_metrics('[');
        assert!(m.height > 0.0);
        assert!(m.depth > 0.0, "bracket should have depth");
    }

    #[test]
    fn brace_lookup() {
        let m = glyph_metrics('{');
        assert!(m.height > 0.0);
        assert!(m.depth > 0.0, "brace should have depth");
    }

    #[test]
    fn arrow_lookup() {
        let m = glyph_metrics('\u{2192}'); // →
        assert!(m.height > 0.0);
        assert!(m.width > 0.0);
    }

    #[test]
    fn italic_correction_for_italic_char() {
        // f typically has italic correction
        let m = glyph_metrics('f');
        // Just verify it doesn't panic and returns valid metrics
        assert!(m.width > 0.0);
    }
}
