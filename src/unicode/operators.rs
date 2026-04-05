//! MathML operator dictionary — O(1) PHF lookup for form, stretchy, and spacing.
//!
//! Spacing values are multiples of 1/18 em, matching the MathML specification:
//!   0 = 0em, 3 = thinmathspace (3/18em), 4 = mediummathspace, 5 = thickmathspace

use phf::phf_map;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorForm {
    Prefix,
    Infix,
    Postfix,
}

/// A single entry in the operator dictionary.
#[derive(Debug, Clone, Copy)]
pub struct OperatorEntry {
    pub character: char,
    pub form: OperatorForm,
    pub stretchy: bool,
    /// Left space in units of 1/18 em.
    pub lspace: u8,
    /// Right space in units of 1/18 em.
    pub rspace: u8,
}

impl OperatorEntry {
    const fn new(ch: char, form: OperatorForm, stretchy: bool, lspace: u8, rspace: u8) -> Self {
        Self {
            character: ch,
            form,
            stretchy,
            lspace,
            rspace,
        }
    }

    /// Left space as an em string for MathML attribute output.
    pub fn lspace_em(&self) -> Option<&'static str> {
        spacing_str(self.lspace)
    }

    /// Right space as an em string for MathML attribute output.
    pub fn rspace_em(&self) -> Option<&'static str> {
        spacing_str(self.rspace)
    }
}

fn spacing_str(val: u8) -> Option<&'static str> {
    match val {
        0 => Some("0em"),
        1 => Some("0.0556em"),
        2 => Some("0.1111em"),
        3 => Some("0.1667em"),
        4 => Some("0.2222em"),
        5 => Some("0.2778em"),
        6 => Some("0.3333em"),
        7 => Some("0.3889em"),
        _ => None,
    }
}

/// Convert form enum to MathML attribute value.
#[inline]
pub fn form_str(form: OperatorForm) -> &'static str {
    match form {
        OperatorForm::Prefix => "prefix",
        OperatorForm::Infix => "infix",
        OperatorForm::Postfix => "postfix",
    }
}

// ---------------------------------------------------------------------------
// PHF maps — one per OperatorForm for O(1) (char, form) lookup.
// ---------------------------------------------------------------------------

static PREFIX_OPS: phf::Map<char, OperatorEntry> = phf_map! {
    // === ASCII ===
    '(' => OperatorEntry::new('(', OperatorForm::Prefix, true, 0, 0),
    '+' => OperatorEntry::new('+', OperatorForm::Prefix, false, 0, 1),
    '-' => OperatorEntry::new('-', OperatorForm::Prefix, false, 0, 1),
    '[' => OperatorEntry::new('[', OperatorForm::Prefix, true, 0, 0),
    '{' => OperatorEntry::new('{', OperatorForm::Prefix, true, 0, 0),
    '|' => OperatorEntry::new('|', OperatorForm::Prefix, true, 0, 0),
    // === Latin-1 Supplement ===
    // ¬ NOT SIGN U+00AC
    '\u{00AC}' => OperatorEntry::new('\u{00AC}', OperatorForm::Prefix, false, 2, 1),
    // ± PLUS-MINUS SIGN U+00B1
    '\u{00B1}' => OperatorEntry::new('\u{00B1}', OperatorForm::Prefix, false, 0, 1),
    // === General Punctuation ===
    // ‖ DOUBLE VERTICAL LINE U+2016
    '\u{2016}' => OperatorEntry::new('\u{2016}', OperatorForm::Prefix, true, 0, 0),
    // === Mathematical Operators ===
    // ∀ FOR ALL U+2200
    '\u{2200}' => OperatorEntry::new('\u{2200}', OperatorForm::Prefix, false, 2, 1),
    // ∂ PARTIAL DIFFERENTIAL U+2202
    '\u{2202}' => OperatorEntry::new('\u{2202}', OperatorForm::Prefix, false, 2, 1),
    // ∃ THERE EXISTS U+2203
    '\u{2203}' => OperatorEntry::new('\u{2203}', OperatorForm::Prefix, false, 2, 1),
    // ∄ THERE DOES NOT EXIST U+2204
    '\u{2204}' => OperatorEntry::new('\u{2204}', OperatorForm::Prefix, false, 2, 1),
    // ∇ NABLA U+2207
    '\u{2207}' => OperatorEntry::new('\u{2207}', OperatorForm::Prefix, false, 2, 1),
    // ∏ N-ARY PRODUCT U+220F
    '\u{220F}' => OperatorEntry::new('\u{220F}', OperatorForm::Prefix, false, 1, 2),
    // ∐ N-ARY COPRODUCT U+2210
    '\u{2210}' => OperatorEntry::new('\u{2210}', OperatorForm::Prefix, false, 1, 2),
    // ∑ N-ARY SUMMATION U+2211
    '\u{2211}' => OperatorEntry::new('\u{2211}', OperatorForm::Prefix, false, 1, 2),
    // − MINUS SIGN U+2212
    '\u{2212}' => OperatorEntry::new('\u{2212}', OperatorForm::Prefix, false, 0, 1),
    // √ SQUARE ROOT U+221A
    '\u{221A}' => OperatorEntry::new('\u{221A}', OperatorForm::Prefix, true, 1, 0),
    // ∫ INTEGRAL U+222B
    '\u{222B}' => OperatorEntry::new('\u{222B}', OperatorForm::Prefix, true, 0, 1),
    // ∬ DOUBLE INTEGRAL U+222C
    '\u{222C}' => OperatorEntry::new('\u{222C}', OperatorForm::Prefix, true, 0, 1),
    // ∭ TRIPLE INTEGRAL U+222D
    '\u{222D}' => OperatorEntry::new('\u{222D}', OperatorForm::Prefix, true, 0, 1),
    // ∮ CONTOUR INTEGRAL U+222E
    '\u{222E}' => OperatorEntry::new('\u{222E}', OperatorForm::Prefix, true, 0, 1),
    // ⋀ N-ARY LOGICAL AND U+22C0
    '\u{22C0}' => OperatorEntry::new('\u{22C0}', OperatorForm::Prefix, false, 1, 2),
    // ⋁ N-ARY LOGICAL OR U+22C1
    '\u{22C1}' => OperatorEntry::new('\u{22C1}', OperatorForm::Prefix, false, 1, 2),
    // ⋂ N-ARY INTERSECTION U+22C2
    '\u{22C2}' => OperatorEntry::new('\u{22C2}', OperatorForm::Prefix, false, 1, 2),
    // ⋃ N-ARY UNION U+22C3
    '\u{22C3}' => OperatorEntry::new('\u{22C3}', OperatorForm::Prefix, false, 1, 2),
    // === Miscellaneous Technical ===
    // ⌈ LEFT CEILING U+2308
    '\u{2308}' => OperatorEntry::new('\u{2308}', OperatorForm::Prefix, true, 0, 0),
    // ⌊ LEFT FLOOR U+230A
    '\u{230A}' => OperatorEntry::new('\u{230A}', OperatorForm::Prefix, true, 0, 0),
    // === Miscellaneous Mathematical Symbols-A ===
    // ⟨ MATHEMATICAL LEFT ANGLE BRACKET U+27E8
    '\u{27E8}' => OperatorEntry::new('\u{27E8}', OperatorForm::Prefix, true, 0, 0),
};

static INFIX_OPS: phf::Map<char, OperatorEntry> = phf_map! {
    // === ASCII ===
    '*' => OperatorEntry::new('*', OperatorForm::Infix, false, 3, 3),
    '+' => OperatorEntry::new('+', OperatorForm::Infix, false, 4, 4),
    ',' => OperatorEntry::new(',', OperatorForm::Infix, false, 0, 3),
    '-' => OperatorEntry::new('-', OperatorForm::Infix, false, 4, 4),
    '.' => OperatorEntry::new('.', OperatorForm::Infix, false, 3, 3),
    '/' => OperatorEntry::new('/', OperatorForm::Infix, true, 1, 1),
    ':' => OperatorEntry::new(':', OperatorForm::Infix, false, 4, 4),
    ';' => OperatorEntry::new(';', OperatorForm::Infix, false, 0, 3),
    '<' => OperatorEntry::new('<', OperatorForm::Infix, false, 5, 5),
    '=' => OperatorEntry::new('=', OperatorForm::Infix, false, 5, 5),
    '>' => OperatorEntry::new('>', OperatorForm::Infix, false, 5, 5),
    '|' => OperatorEntry::new('|', OperatorForm::Infix, true, 5, 5),
    // === Latin-1 Supplement ===
    // ± PLUS-MINUS SIGN U+00B1
    '\u{00B1}' => OperatorEntry::new('\u{00B1}', OperatorForm::Infix, false, 4, 4),
    // × MULTIPLICATION SIGN U+00D7
    '\u{00D7}' => OperatorEntry::new('\u{00D7}', OperatorForm::Infix, false, 4, 4),
    // ÷ DIVISION SIGN U+00F7
    '\u{00F7}' => OperatorEntry::new('\u{00F7}', OperatorForm::Infix, false, 4, 4),
    // === Arrows U+2190–U+21FF ===
    // ← LEFTWARDS ARROW U+2190
    '\u{2190}' => OperatorEntry::new('\u{2190}', OperatorForm::Infix, true, 5, 5),
    // ↑ UPWARDS ARROW U+2191
    '\u{2191}' => OperatorEntry::new('\u{2191}', OperatorForm::Infix, true, 5, 5),
    // → RIGHTWARDS ARROW U+2192
    '\u{2192}' => OperatorEntry::new('\u{2192}', OperatorForm::Infix, true, 5, 5),
    // ↓ DOWNWARDS ARROW U+2193
    '\u{2193}' => OperatorEntry::new('\u{2193}', OperatorForm::Infix, true, 5, 5),
    // ↔ LEFT RIGHT ARROW U+2194
    '\u{2194}' => OperatorEntry::new('\u{2194}', OperatorForm::Infix, true, 5, 5),
    // ↕ UP DOWN ARROW U+2195
    '\u{2195}' => OperatorEntry::new('\u{2195}', OperatorForm::Infix, true, 5, 5),
    // ↦ RIGHTWARDS ARROW FROM BAR U+21A6 (mapsto)
    '\u{21A6}' => OperatorEntry::new('\u{21A6}', OperatorForm::Infix, true, 5, 5),
    // ⇐ LEFTWARDS DOUBLE ARROW U+21D0
    '\u{21D0}' => OperatorEntry::new('\u{21D0}', OperatorForm::Infix, true, 5, 5),
    // ⇑ UPWARDS DOUBLE ARROW U+21D1
    '\u{21D1}' => OperatorEntry::new('\u{21D1}', OperatorForm::Infix, true, 5, 5),
    // ⇒ RIGHTWARDS DOUBLE ARROW U+21D2 (implies)
    '\u{21D2}' => OperatorEntry::new('\u{21D2}', OperatorForm::Infix, true, 5, 5),
    // ⇓ DOWNWARDS DOUBLE ARROW U+21D3
    '\u{21D3}' => OperatorEntry::new('\u{21D3}', OperatorForm::Infix, true, 5, 5),
    // ⇔ LEFT RIGHT DOUBLE ARROW U+21D4 (iff)
    '\u{21D4}' => OperatorEntry::new('\u{21D4}', OperatorForm::Infix, true, 5, 5),
    // === Mathematical Operators U+2200–U+22FF ===
    // ∈ ELEMENT OF U+2208
    '\u{2208}' => OperatorEntry::new('\u{2208}', OperatorForm::Infix, false, 5, 5),
    // ∉ NOT AN ELEMENT OF U+2209
    '\u{2209}' => OperatorEntry::new('\u{2209}', OperatorForm::Infix, false, 5, 5),
    // ∋ CONTAINS AS MEMBER U+220B
    '\u{220B}' => OperatorEntry::new('\u{220B}', OperatorForm::Infix, false, 5, 5),
    // − MINUS SIGN U+2212
    '\u{2212}' => OperatorEntry::new('\u{2212}', OperatorForm::Infix, false, 4, 4),
    // ∓ MINUS-OR-PLUS SIGN U+2213
    '\u{2213}' => OperatorEntry::new('\u{2213}', OperatorForm::Infix, false, 4, 4),
    // ∘ RING OPERATOR U+2218
    '\u{2218}' => OperatorEntry::new('\u{2218}', OperatorForm::Infix, false, 3, 3),
    // ∙ BULLET OPERATOR U+2219
    '\u{2219}' => OperatorEntry::new('\u{2219}', OperatorForm::Infix, false, 3, 3),
    // ∝ PROPORTIONAL TO U+221D
    '\u{221D}' => OperatorEntry::new('\u{221D}', OperatorForm::Infix, false, 5, 5),
    // ∞ INFINITY U+221E
    '\u{221E}' => OperatorEntry::new('\u{221E}', OperatorForm::Infix, false, 5, 5),
    // ∣ DIVIDES U+2223
    '\u{2223}' => OperatorEntry::new('\u{2223}', OperatorForm::Infix, false, 5, 5),
    // ∥ PARALLEL TO U+2225
    '\u{2225}' => OperatorEntry::new('\u{2225}', OperatorForm::Infix, false, 5, 5),
    // ∧ LOGICAL AND U+2227
    '\u{2227}' => OperatorEntry::new('\u{2227}', OperatorForm::Infix, false, 4, 4),
    // ∨ LOGICAL OR U+2228
    '\u{2228}' => OperatorEntry::new('\u{2228}', OperatorForm::Infix, false, 4, 4),
    // ∩ INTERSECTION U+2229
    '\u{2229}' => OperatorEntry::new('\u{2229}', OperatorForm::Infix, false, 4, 4),
    // ∪ UNION U+222A
    '\u{222A}' => OperatorEntry::new('\u{222A}', OperatorForm::Infix, false, 4, 4),
    // ∴ THEREFORE U+2234
    '\u{2234}' => OperatorEntry::new('\u{2234}', OperatorForm::Infix, false, 5, 5),
    // ∵ BECAUSE U+2235
    '\u{2235}' => OperatorEntry::new('\u{2235}', OperatorForm::Infix, false, 5, 5),
    // ∼ TILDE OPERATOR U+223C
    '\u{223C}' => OperatorEntry::new('\u{223C}', OperatorForm::Infix, false, 5, 5),
    // ≃ ASYMPTOTICALLY EQUAL TO U+2243
    '\u{2243}' => OperatorEntry::new('\u{2243}', OperatorForm::Infix, false, 5, 5),
    // ≅ APPROXIMATELY EQUAL TO U+2245
    '\u{2245}' => OperatorEntry::new('\u{2245}', OperatorForm::Infix, false, 5, 5),
    // ≈ ALMOST EQUAL TO U+2248
    '\u{2248}' => OperatorEntry::new('\u{2248}', OperatorForm::Infix, false, 5, 5),
    // ≠ NOT EQUAL TO U+2260
    '\u{2260}' => OperatorEntry::new('\u{2260}', OperatorForm::Infix, false, 5, 5),
    // ≡ IDENTICAL TO U+2261
    '\u{2261}' => OperatorEntry::new('\u{2261}', OperatorForm::Infix, false, 5, 5),
    // ≤ LESS-THAN OR EQUAL TO U+2264
    '\u{2264}' => OperatorEntry::new('\u{2264}', OperatorForm::Infix, false, 5, 5),
    // ≥ GREATER-THAN OR EQUAL TO U+2265
    '\u{2265}' => OperatorEntry::new('\u{2265}', OperatorForm::Infix, false, 5, 5),
    // ≪ MUCH LESS-THAN U+226A
    '\u{226A}' => OperatorEntry::new('\u{226A}', OperatorForm::Infix, false, 5, 5),
    // ≫ MUCH GREATER-THAN U+226B
    '\u{226B}' => OperatorEntry::new('\u{226B}', OperatorForm::Infix, false, 5, 5),
    // ≺ PRECEDES U+227A
    '\u{227A}' => OperatorEntry::new('\u{227A}', OperatorForm::Infix, false, 5, 5),
    // ≻ SUCCEEDS U+227B
    '\u{227B}' => OperatorEntry::new('\u{227B}', OperatorForm::Infix, false, 5, 5),
    // ⊂ SUBSET OF U+2282
    '\u{2282}' => OperatorEntry::new('\u{2282}', OperatorForm::Infix, false, 5, 5),
    // ⊃ SUPERSET OF U+2283
    '\u{2283}' => OperatorEntry::new('\u{2283}', OperatorForm::Infix, false, 5, 5),
    // ⊆ SUBSET OF OR EQUAL TO U+2286
    '\u{2286}' => OperatorEntry::new('\u{2286}', OperatorForm::Infix, false, 5, 5),
    // ⊇ SUPERSET OF OR EQUAL TO U+2287
    '\u{2287}' => OperatorEntry::new('\u{2287}', OperatorForm::Infix, false, 5, 5),
    // ⊕ CIRCLED PLUS U+2295
    '\u{2295}' => OperatorEntry::new('\u{2295}', OperatorForm::Infix, false, 4, 4),
    // ⊖ CIRCLED MINUS U+2296
    '\u{2296}' => OperatorEntry::new('\u{2296}', OperatorForm::Infix, false, 4, 4),
    // ⊗ CIRCLED TIMES U+2297
    '\u{2297}' => OperatorEntry::new('\u{2297}', OperatorForm::Infix, false, 4, 4),
    // ⊘ CIRCLED DIVISION SLASH U+2298
    '\u{2298}' => OperatorEntry::new('\u{2298}', OperatorForm::Infix, false, 4, 4),
    // ⊙ CIRCLED DOT OPERATOR U+2299
    '\u{2299}' => OperatorEntry::new('\u{2299}', OperatorForm::Infix, false, 4, 4),
    // ⊢ RIGHT TACK U+22A2 (vdash)
    '\u{22A2}' => OperatorEntry::new('\u{22A2}', OperatorForm::Infix, false, 5, 5),
    // ⊣ LEFT TACK U+22A3 (dashv)
    '\u{22A3}' => OperatorEntry::new('\u{22A3}', OperatorForm::Infix, false, 5, 5),
    // ⊤ DOWN TACK U+22A4 (top)
    '\u{22A4}' => OperatorEntry::new('\u{22A4}', OperatorForm::Infix, false, 5, 5),
    // ⊥ UP TACK U+22A5 (bot/perp)
    '\u{22A5}' => OperatorEntry::new('\u{22A5}', OperatorForm::Infix, false, 5, 5),
    // ⊨ MODELS U+22A8 (double turnstile)
    '\u{22A8}' => OperatorEntry::new('\u{22A8}', OperatorForm::Infix, false, 5, 5),
    // ⋅ DOT OPERATOR U+22C5
    '\u{22C5}' => OperatorEntry::new('\u{22C5}', OperatorForm::Infix, false, 3, 3),
    // ⋯ MIDLINE HORIZONTAL ELLIPSIS U+22EF
    '\u{22EF}' => OperatorEntry::new('\u{22EF}', OperatorForm::Infix, false, 0, 0),
    // === Supplemental Arrows-A U+27F0–U+27FF ===
    // ⟵ LONG LEFTWARDS ARROW U+27F5
    '\u{27F5}' => OperatorEntry::new('\u{27F5}', OperatorForm::Infix, true, 5, 5),
    // ⟶ LONG RIGHTWARDS ARROW U+27F6
    '\u{27F6}' => OperatorEntry::new('\u{27F6}', OperatorForm::Infix, true, 5, 5),
    // ⟸ LONG LEFTWARDS DOUBLE ARROW U+27F8
    '\u{27F8}' => OperatorEntry::new('\u{27F8}', OperatorForm::Infix, true, 5, 5),
    // ⟹ LONG RIGHTWARDS DOUBLE ARROW U+27F9
    '\u{27F9}' => OperatorEntry::new('\u{27F9}', OperatorForm::Infix, true, 5, 5),
};

static POSTFIX_OPS: phf::Map<char, OperatorEntry> = phf_map! {
    '!' => OperatorEntry::new('!', OperatorForm::Postfix, false, 1, 0),
    ')' => OperatorEntry::new(')', OperatorForm::Postfix, true, 0, 0),
    ']' => OperatorEntry::new(']', OperatorForm::Postfix, true, 0, 0),
    '|' => OperatorEntry::new('|', OperatorForm::Postfix, true, 0, 0),
    '}' => OperatorEntry::new('}', OperatorForm::Postfix, true, 0, 0),
    // ‖ DOUBLE VERTICAL LINE U+2016
    '\u{2016}' => OperatorEntry::new('\u{2016}', OperatorForm::Postfix, true, 0, 0),
    // ⌉ RIGHT CEILING U+2309
    '\u{2309}' => OperatorEntry::new('\u{2309}', OperatorForm::Postfix, true, 0, 0),
    // ⌋ RIGHT FLOOR U+230B
    '\u{230B}' => OperatorEntry::new('\u{230B}', OperatorForm::Postfix, true, 0, 0),
    // ⟩ MATHEMATICAL RIGHT ANGLE BRACKET U+27E9
    '\u{27E9}' => OperatorEntry::new('\u{27E9}', OperatorForm::Postfix, true, 0, 0),
};

// ---------------------------------------------------------------------------
// Public API — same signatures as before, backed by PHF instead of binary search.
// ---------------------------------------------------------------------------

/// Look up an operator entry by character and form. O(1) via PHF.
#[inline]
pub fn lookup_operator(ch: char, form: OperatorForm) -> Option<&'static OperatorEntry> {
    match form {
        OperatorForm::Prefix => PREFIX_OPS.get(&ch),
        OperatorForm::Infix => INFIX_OPS.get(&ch),
        OperatorForm::Postfix => POSTFIX_OPS.get(&ch),
    }
}

/// Look up an operator entry by character, trying the given form first,
/// then falling back in priority order: Infix > Prefix > Postfix.
/// Each form is checked at most once.
#[inline]
pub fn lookup_operator_any(ch: char, preferred: OperatorForm) -> Option<&'static OperatorEntry> {
    lookup_operator(ch, preferred).or_else(|| {
        [OperatorForm::Infix, OperatorForm::Prefix, OperatorForm::Postfix]
            .iter()
            .filter(|&&f| f != preferred)
            .find_map(|&f| lookup_operator(ch, f))
    })
}

/// Determine if a character is a stretchy operator in some form.
#[inline]
pub fn is_stretchy(ch: char) -> bool {
    PREFIX_OPS.get(&ch).is_some_and(|e| e.stretchy)
        || INFIX_OPS.get(&ch).is_some_and(|e| e.stretchy)
        || POSTFIX_OPS.get(&ch).is_some_and(|e| e.stretchy)
}

/// Infer operator form from the AST symbol kind.
#[inline]
pub fn symbol_kind_to_form(kind: crate::ast::SymbolKind) -> OperatorForm {
    match kind {
        crate::ast::SymbolKind::Open | crate::ast::SymbolKind::Fence => OperatorForm::Prefix,
        crate::ast::SymbolKind::Close => OperatorForm::Postfix,
        _ => OperatorForm::Infix,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_basic() {
        let entry = lookup_operator('+', OperatorForm::Infix).unwrap();
        assert_eq!(entry.lspace, 4);
        assert_eq!(entry.rspace, 4);
        assert!(!entry.stretchy);
    }

    #[test]
    fn lookup_paren() {
        let open = lookup_operator('(', OperatorForm::Prefix).unwrap();
        assert!(open.stretchy);
        assert_eq!(open.lspace, 0);

        let close = lookup_operator(')', OperatorForm::Postfix).unwrap();
        assert!(close.stretchy);
    }

    #[test]
    fn lookup_integral() {
        let entry = lookup_operator('\u{222B}', OperatorForm::Prefix).unwrap();
        assert!(entry.stretchy);
    }

    #[test]
    fn lookup_fallback() {
        let entry = lookup_operator_any('=', OperatorForm::Prefix);
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().form, OperatorForm::Infix);
    }

    #[test]
    fn lookup_fallback_multimap_prefers_infix() {
        // '+' has both Prefix and Infix entries; fallback should prefer Infix.
        let entry = lookup_operator_any('+', OperatorForm::Postfix).unwrap();
        assert_eq!(entry.form, OperatorForm::Infix);
    }

    #[test]
    fn stretchy_check() {
        assert!(is_stretchy('('));
        assert!(is_stretchy('\u{222B}'));
        assert!(!is_stretchy('+'));
    }

    #[test]
    fn phf_maps_cover_all_operators() {
        let test_cases: &[(char, OperatorForm, bool, u8, u8)] = &[
            ('(', OperatorForm::Prefix, true, 0, 0),
            (')', OperatorForm::Postfix, true, 0, 0),
            ('+', OperatorForm::Prefix, false, 0, 1),
            ('+', OperatorForm::Infix, false, 4, 4),
            ('-', OperatorForm::Prefix, false, 0, 1),
            ('-', OperatorForm::Infix, false, 4, 4),
            ('|', OperatorForm::Prefix, true, 0, 0),
            ('|', OperatorForm::Infix, true, 5, 5),
            ('|', OperatorForm::Postfix, true, 0, 0),
            ('\u{2211}', OperatorForm::Prefix, false, 1, 2),
            ('\u{222B}', OperatorForm::Prefix, true, 0, 1),
            ('\u{2192}', OperatorForm::Infix, true, 5, 5),
            ('\u{21D2}', OperatorForm::Infix, true, 5, 5),
            ('\u{27E8}', OperatorForm::Prefix, true, 0, 0),
            ('\u{27E9}', OperatorForm::Postfix, true, 0, 0),
            ('!', OperatorForm::Postfix, false, 1, 0),
        ];
        for &(ch, form, stretchy, lspace, rspace) in test_cases {
            let entry = lookup_operator(ch, form)
                .unwrap_or_else(|| panic!("missing operator {:?} {:?}", ch, form));
            assert_eq!(entry.character, ch, "char mismatch for {:?} {:?}", ch, form);
            assert_eq!(entry.form, form, "form mismatch for {:?}", ch);
            assert_eq!(
                entry.stretchy, stretchy,
                "stretchy mismatch for {:?} {:?}",
                ch, form
            );
            assert_eq!(entry.lspace, lspace, "lspace mismatch for {:?} {:?}", ch, form);
            assert_eq!(entry.rspace, rspace, "rspace mismatch for {:?} {:?}", ch, form);
        }
    }

    #[test]
    fn phf_entry_count() {
        // Verify the PHF maps collectively have the same count as the old sorted array.
        let prefix = PREFIX_OPS.len();
        let infix = INFIX_OPS.len();
        let postfix = POSTFIX_OPS.len();
        assert_eq!(prefix, 30, "prefix count");
        assert_eq!(infix, 76, "infix count");
        assert_eq!(postfix, 9, "postfix count");
        assert_eq!(prefix + infix + postfix, 115, "total operator count changed");
    }
}
