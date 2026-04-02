use crate::ast::SymbolKind;

/// A symbol entry mapping a LaTeX command to a Unicode character and its math class.
#[derive(Debug, Clone, Copy)]
pub struct SymbolEntry {
    pub command: &'static str,
    pub unicode: char,
    pub kind: SymbolKind,
}

/// Look up a Unicode character by LaTeX command name (without backslash).
/// Uses binary search on the sorted symbol table.
pub fn latex_to_unicode(command: &str) -> Option<char> {
    SYMBOLS
        .binary_search_by_key(&command, |e| e.command)
        .ok()
        .map(|i| SYMBOLS[i].unicode)
}

/// Look up a LaTeX command name for a Unicode character.
pub fn unicode_to_latex(ch: char) -> Option<&'static str> {
    SYMBOLS
        .iter()
        .find(|e| e.unicode == ch)
        .map(|e| e.command)
}

/// Look up the math symbol kind for a Unicode character.
pub fn symbol_kind(ch: char) -> SymbolKind {
    SYMBOLS
        .iter()
        .find(|e| e.unicode == ch)
        .map(|e| e.kind)
        .unwrap_or(SymbolKind::Ordinary)
}

/// Look up a symbol entry by LaTeX command name.
/// Uses binary search on the sorted symbol table.
pub fn lookup_command(command: &str) -> Option<&'static SymbolEntry> {
    SYMBOLS
        .binary_search_by_key(&command, |e| e.command)
        .ok()
        .map(|i| &SYMBOLS[i])
}

use SymbolKind::*;

/// Symbol table sorted by command name for binary search.
/// IMPORTANT: Entries MUST be sorted by `command` in Rust's default string ordering.
pub static SYMBOLS: &[SymbolEntry] = &[
    // === Uppercase commands (ASCII uppercase < lowercase) ===
    SymbolEntry { command: "Delta", unicode: '\u{0394}', kind: Alphabetic },
    SymbolEntry { command: "Downarrow", unicode: '\u{21D3}', kind: Relation },
    SymbolEntry { command: "Gamma", unicode: '\u{0393}', kind: Alphabetic },
    SymbolEntry { command: "Im", unicode: '\u{2111}', kind: Ordinary },
    SymbolEntry { command: "Lambda", unicode: '\u{039B}', kind: Alphabetic },
    SymbolEntry { command: "Leftarrow", unicode: '\u{21D0}', kind: Relation },
    SymbolEntry { command: "Leftrightarrow", unicode: '\u{21D4}', kind: Relation },
    SymbolEntry { command: "Longleftarrow", unicode: '\u{27F8}', kind: Relation },
    SymbolEntry { command: "Longrightarrow", unicode: '\u{27F9}', kind: Relation },
    SymbolEntry { command: "Omega", unicode: '\u{03A9}', kind: Alphabetic },
    SymbolEntry { command: "Phi", unicode: '\u{03A6}', kind: Alphabetic },
    SymbolEntry { command: "Pi", unicode: '\u{03A0}', kind: Alphabetic },
    SymbolEntry { command: "Psi", unicode: '\u{03A8}', kind: Alphabetic },
    SymbolEntry { command: "Re", unicode: '\u{211C}', kind: Ordinary },
    SymbolEntry { command: "Rightarrow", unicode: '\u{21D2}', kind: Relation },
    SymbolEntry { command: "Sigma", unicode: '\u{03A3}', kind: Alphabetic },
    SymbolEntry { command: "Theta", unicode: '\u{0398}', kind: Alphabetic },
    SymbolEntry { command: "Uparrow", unicode: '\u{21D1}', kind: Relation },
    SymbolEntry { command: "Updownarrow", unicode: '\u{21D5}', kind: Relation },
    SymbolEntry { command: "Upsilon", unicode: '\u{03A5}', kind: Alphabetic },
    SymbolEntry { command: "Xi", unicode: '\u{039E}', kind: Alphabetic },

    // === Lowercase commands ===
    SymbolEntry { command: "aleph", unicode: '\u{2135}', kind: Ordinary },
    SymbolEntry { command: "alpha", unicode: '\u{03B1}', kind: Alphabetic },
    SymbolEntry { command: "amalg", unicode: '\u{2A3F}', kind: BinaryOp },
    SymbolEntry { command: "angle", unicode: '\u{2220}', kind: Ordinary },
    SymbolEntry { command: "approx", unicode: '\u{2248}', kind: Relation },
    SymbolEntry { command: "ast", unicode: '\u{2217}', kind: BinaryOp },
    SymbolEntry { command: "asymp", unicode: '\u{224D}', kind: Relation },
    SymbolEntry { command: "backprime", unicode: '\u{2035}', kind: Ordinary },
    SymbolEntry { command: "because", unicode: '\u{2235}', kind: Relation },
    SymbolEntry { command: "beta", unicode: '\u{03B2}', kind: Alphabetic },
    SymbolEntry { command: "beth", unicode: '\u{2136}', kind: Ordinary },
    SymbolEntry { command: "bigcap", unicode: '\u{22C2}', kind: LargeOp },
    SymbolEntry { command: "bigcup", unicode: '\u{22C3}', kind: LargeOp },
    SymbolEntry { command: "bigodot", unicode: '\u{2A00}', kind: LargeOp },
    SymbolEntry { command: "bigoplus", unicode: '\u{2A01}', kind: LargeOp },
    SymbolEntry { command: "bigotimes", unicode: '\u{2A02}', kind: LargeOp },
    SymbolEntry { command: "bigsqcup", unicode: '\u{2A06}', kind: LargeOp },
    SymbolEntry { command: "bigtriangledown", unicode: '\u{25BD}', kind: BinaryOp },
    SymbolEntry { command: "bigtriangleup", unicode: '\u{25B3}', kind: BinaryOp },
    SymbolEntry { command: "biguplus", unicode: '\u{2A04}', kind: LargeOp },
    SymbolEntry { command: "bigvee", unicode: '\u{22C1}', kind: LargeOp },
    SymbolEntry { command: "bigwedge", unicode: '\u{22C0}', kind: LargeOp },
    SymbolEntry { command: "bot", unicode: '\u{22A5}', kind: Ordinary },
    SymbolEntry { command: "bowtie", unicode: '\u{22C8}', kind: Relation },
    SymbolEntry { command: "bullet", unicode: '\u{2219}', kind: BinaryOp },
    SymbolEntry { command: "cap", unicode: '\u{2229}', kind: BinaryOp },
    SymbolEntry { command: "cdot", unicode: '\u{22C5}', kind: BinaryOp },
    SymbolEntry { command: "cdots", unicode: '\u{22EF}', kind: Ordinary },
    SymbolEntry { command: "chi", unicode: '\u{03C7}', kind: Alphabetic },
    SymbolEntry { command: "circ", unicode: '\u{2218}', kind: BinaryOp },
    SymbolEntry { command: "clubsuit", unicode: '\u{2663}', kind: Ordinary },
    SymbolEntry { command: "colon", unicode: ':', kind: Punctuation },
    SymbolEntry { command: "complement", unicode: '\u{2201}', kind: Ordinary },
    SymbolEntry { command: "cong", unicode: '\u{2245}', kind: Relation },
    SymbolEntry { command: "coprod", unicode: '\u{2210}', kind: LargeOp },
    SymbolEntry { command: "cup", unicode: '\u{222A}', kind: BinaryOp },
    SymbolEntry { command: "dagger", unicode: '\u{2020}', kind: BinaryOp },
    SymbolEntry { command: "daleth", unicode: '\u{2138}', kind: Ordinary },
    SymbolEntry { command: "dashv", unicode: '\u{22A3}', kind: Relation },
    SymbolEntry { command: "ddagger", unicode: '\u{2021}', kind: BinaryOp },
    SymbolEntry { command: "ddots", unicode: '\u{22F1}', kind: Ordinary },
    SymbolEntry { command: "degree", unicode: '\u{00B0}', kind: Ordinary },
    SymbolEntry { command: "delta", unicode: '\u{03B4}', kind: Alphabetic },
    SymbolEntry { command: "diamond", unicode: '\u{22C4}', kind: BinaryOp },
    SymbolEntry { command: "diamondsuit", unicode: '\u{2666}', kind: Ordinary },
    SymbolEntry { command: "div", unicode: '\u{00F7}', kind: BinaryOp },
    SymbolEntry { command: "doteq", unicode: '\u{2250}', kind: Relation },
    SymbolEntry { command: "dots", unicode: '\u{2026}', kind: Ordinary },
    SymbolEntry { command: "downarrow", unicode: '\u{2193}', kind: Relation },
    SymbolEntry { command: "ell", unicode: '\u{2113}', kind: Ordinary },
    SymbolEntry { command: "emptyset", unicode: '\u{2205}', kind: Ordinary },
    SymbolEntry { command: "epsilon", unicode: '\u{03F5}', kind: Alphabetic },
    SymbolEntry { command: "equiv", unicode: '\u{2261}', kind: Relation },
    SymbolEntry { command: "eta", unicode: '\u{03B7}', kind: Alphabetic },
    SymbolEntry { command: "eth", unicode: '\u{00F0}', kind: Ordinary },
    SymbolEntry { command: "exists", unicode: '\u{2203}', kind: Ordinary },
    SymbolEntry { command: "flat", unicode: '\u{266D}', kind: Ordinary },
    SymbolEntry { command: "forall", unicode: '\u{2200}', kind: Ordinary },
    SymbolEntry { command: "gamma", unicode: '\u{03B3}', kind: Alphabetic },
    SymbolEntry { command: "ge", unicode: '\u{2265}', kind: Relation },
    SymbolEntry { command: "geq", unicode: '\u{2265}', kind: Relation },
    SymbolEntry { command: "geqslant", unicode: '\u{2A7E}', kind: Relation },
    SymbolEntry { command: "gg", unicode: '\u{226B}', kind: Relation },
    SymbolEntry { command: "gimel", unicode: '\u{2137}', kind: Ordinary },
    SymbolEntry { command: "hbar", unicode: '\u{210F}', kind: Ordinary },
    SymbolEntry { command: "heartsuit", unicode: '\u{2665}', kind: Ordinary },
    SymbolEntry { command: "hookleftarrow", unicode: '\u{21A9}', kind: Relation },
    SymbolEntry { command: "hookrightarrow", unicode: '\u{21AA}', kind: Relation },
    SymbolEntry { command: "iff", unicode: '\u{27FA}', kind: Relation },
    SymbolEntry { command: "iiint", unicode: '\u{222D}', kind: LargeOp },
    SymbolEntry { command: "iint", unicode: '\u{222C}', kind: LargeOp },
    SymbolEntry { command: "imath", unicode: '\u{0131}', kind: Ordinary },
    SymbolEntry { command: "implies", unicode: '\u{27F9}', kind: Relation },

    SymbolEntry { command: "in", unicode: '\u{2208}', kind: Relation },
    SymbolEntry { command: "infty", unicode: '\u{221E}', kind: Ordinary },
    SymbolEntry { command: "int", unicode: '\u{222B}', kind: LargeOp },
    SymbolEntry { command: "iota", unicode: '\u{03B9}', kind: Alphabetic },
    SymbolEntry { command: "jmath", unicode: '\u{0237}', kind: Ordinary },
    SymbolEntry { command: "kappa", unicode: '\u{03BA}', kind: Alphabetic },
    SymbolEntry { command: "lVert", unicode: '\u{2016}', kind: Open },
    SymbolEntry { command: "lambda", unicode: '\u{03BB}', kind: Alphabetic },
    SymbolEntry { command: "land", unicode: '\u{2227}', kind: BinaryOp },
    SymbolEntry { command: "langle", unicode: '\u{27E8}', kind: Open },
    SymbolEntry { command: "lbrace", unicode: '{', kind: Open },
    SymbolEntry { command: "lbrack", unicode: '[', kind: Open },
    SymbolEntry { command: "lceil", unicode: '\u{2308}', kind: Open },
    SymbolEntry { command: "ldots", unicode: '\u{2026}', kind: Ordinary },
    SymbolEntry { command: "le", unicode: '\u{2264}', kind: Relation },
    SymbolEntry { command: "leftarrow", unicode: '\u{2190}', kind: Relation },
    SymbolEntry { command: "leftharpoondown", unicode: '\u{21BD}', kind: Relation },
    SymbolEntry { command: "leftharpoonup", unicode: '\u{21BC}', kind: Relation },
    SymbolEntry { command: "leftrightarrow", unicode: '\u{2194}', kind: Relation },
    SymbolEntry { command: "leq", unicode: '\u{2264}', kind: Relation },
    SymbolEntry { command: "leqslant", unicode: '\u{2A7D}', kind: Relation },
    SymbolEntry { command: "lfloor", unicode: '\u{230A}', kind: Open },
    SymbolEntry { command: "ll", unicode: '\u{226A}', kind: Relation },
    SymbolEntry { command: "lnot", unicode: '\u{00AC}', kind: Ordinary },
    SymbolEntry { command: "longleftarrow", unicode: '\u{27F5}', kind: Relation },
    SymbolEntry { command: "longleftrightarrow", unicode: '\u{27F7}', kind: Relation },
    SymbolEntry { command: "longmapsto", unicode: '\u{27FC}', kind: Relation },
    SymbolEntry { command: "longrightarrow", unicode: '\u{27F6}', kind: Relation },
    SymbolEntry { command: "lor", unicode: '\u{2228}', kind: BinaryOp },
    SymbolEntry { command: "lvert", unicode: '|', kind: Open },
    SymbolEntry { command: "mapsto", unicode: '\u{21A6}', kind: Relation },
    SymbolEntry { command: "measuredangle", unicode: '\u{2221}', kind: Ordinary },
    SymbolEntry { command: "mid", unicode: '\u{2223}', kind: Relation },
    SymbolEntry { command: "models", unicode: '\u{22A8}', kind: Relation },
    SymbolEntry { command: "mp", unicode: '\u{2213}', kind: BinaryOp },
    SymbolEntry { command: "mu", unicode: '\u{03BC}', kind: Alphabetic },
    SymbolEntry { command: "nabla", unicode: '\u{2207}', kind: Ordinary },
    SymbolEntry { command: "natural", unicode: '\u{266E}', kind: Ordinary },
    SymbolEntry { command: "ne", unicode: '\u{2260}', kind: Relation },
    SymbolEntry { command: "nearrow", unicode: '\u{2197}', kind: Relation },
    SymbolEntry { command: "neg", unicode: '\u{00AC}', kind: Ordinary },
    SymbolEntry { command: "neq", unicode: '\u{2260}', kind: Relation },
    SymbolEntry { command: "nexists", unicode: '\u{2204}', kind: Ordinary },
    SymbolEntry { command: "ngeq", unicode: '\u{2271}', kind: Relation },
    SymbolEntry { command: "ni", unicode: '\u{220B}', kind: Relation },
    SymbolEntry { command: "nleq", unicode: '\u{2270}', kind: Relation },
    SymbolEntry { command: "nmid", unicode: '\u{2224}', kind: Relation },
    SymbolEntry { command: "notin", unicode: '\u{2209}', kind: Relation },
    SymbolEntry { command: "nprec", unicode: '\u{2280}', kind: Relation },
    SymbolEntry { command: "nsubseteq", unicode: '\u{2288}', kind: Relation },
    SymbolEntry { command: "nsucc", unicode: '\u{2281}', kind: Relation },
    SymbolEntry { command: "nsupseteq", unicode: '\u{2289}', kind: Relation },
    SymbolEntry { command: "nu", unicode: '\u{03BD}', kind: Alphabetic },
    SymbolEntry { command: "nwarrow", unicode: '\u{2196}', kind: Relation },
    SymbolEntry { command: "odot", unicode: '\u{2299}', kind: BinaryOp },
    SymbolEntry { command: "oint", unicode: '\u{222E}', kind: LargeOp },
    SymbolEntry { command: "omega", unicode: '\u{03C9}', kind: Alphabetic },
    SymbolEntry { command: "ominus", unicode: '\u{2296}', kind: BinaryOp },
    SymbolEntry { command: "oplus", unicode: '\u{2295}', kind: BinaryOp },
    SymbolEntry { command: "oslash", unicode: '\u{2298}', kind: BinaryOp },
    SymbolEntry { command: "otimes", unicode: '\u{2297}', kind: BinaryOp },
    SymbolEntry { command: "parallel", unicode: '\u{2225}', kind: Relation },
    SymbolEntry { command: "partial", unicode: '\u{2202}', kind: Ordinary },
    SymbolEntry { command: "perp", unicode: '\u{22A5}', kind: Relation },
    SymbolEntry { command: "phi", unicode: '\u{03D5}', kind: Alphabetic },
    SymbolEntry { command: "pi", unicode: '\u{03C0}', kind: Alphabetic },
    SymbolEntry { command: "pm", unicode: '\u{00B1}', kind: BinaryOp },
    SymbolEntry { command: "prec", unicode: '\u{227A}', kind: Relation },
    SymbolEntry { command: "preceq", unicode: '\u{2AAF}', kind: Relation },
    SymbolEntry { command: "prime", unicode: '\u{2032}', kind: Ordinary },
    SymbolEntry { command: "prod", unicode: '\u{220F}', kind: LargeOp },
    SymbolEntry { command: "propto", unicode: '\u{221D}', kind: Relation },
    SymbolEntry { command: "psi", unicode: '\u{03C8}', kind: Alphabetic },
    SymbolEntry { command: "rVert", unicode: '\u{2016}', kind: Close },
    SymbolEntry { command: "rangle", unicode: '\u{27E9}', kind: Close },
    SymbolEntry { command: "rbrace", unicode: '}', kind: Close },
    SymbolEntry { command: "rbrack", unicode: ']', kind: Close },
    SymbolEntry { command: "rceil", unicode: '\u{2309}', kind: Close },
    SymbolEntry { command: "rfloor", unicode: '\u{230B}', kind: Close },
    SymbolEntry { command: "rho", unicode: '\u{03C1}', kind: Alphabetic },
    SymbolEntry { command: "rightarrow", unicode: '\u{2192}', kind: Relation },
    SymbolEntry { command: "rightharpoondown", unicode: '\u{21C1}', kind: Relation },
    SymbolEntry { command: "rightharpoonup", unicode: '\u{21C0}', kind: Relation },
    SymbolEntry { command: "rightleftharpoons", unicode: '\u{21CC}', kind: Relation },
    SymbolEntry { command: "rvert", unicode: '|', kind: Close },
    SymbolEntry { command: "searrow", unicode: '\u{2198}', kind: Relation },
    SymbolEntry { command: "setminus", unicode: '\u{2216}', kind: BinaryOp },
    SymbolEntry { command: "sharp", unicode: '\u{266F}', kind: Ordinary },
    SymbolEntry { command: "sigma", unicode: '\u{03C3}', kind: Alphabetic },
    SymbolEntry { command: "sim", unicode: '\u{223C}', kind: Relation },
    SymbolEntry { command: "simeq", unicode: '\u{2243}', kind: Relation },
    SymbolEntry { command: "spadesuit", unicode: '\u{2660}', kind: Ordinary },
    SymbolEntry { command: "sqcap", unicode: '\u{2293}', kind: BinaryOp },
    SymbolEntry { command: "sqcup", unicode: '\u{2294}', kind: BinaryOp },
    SymbolEntry { command: "sqsubseteq", unicode: '\u{2291}', kind: Relation },
    SymbolEntry { command: "sqsupseteq", unicode: '\u{2292}', kind: Relation },
    SymbolEntry { command: "square", unicode: '\u{25A1}', kind: Ordinary },
    SymbolEntry { command: "star", unicode: '\u{22C6}', kind: BinaryOp },
    SymbolEntry { command: "subset", unicode: '\u{2282}', kind: Relation },
    SymbolEntry { command: "subseteq", unicode: '\u{2286}', kind: Relation },
    SymbolEntry { command: "succ", unicode: '\u{227B}', kind: Relation },
    SymbolEntry { command: "succeq", unicode: '\u{2AB0}', kind: Relation },
    SymbolEntry { command: "sum", unicode: '\u{2211}', kind: LargeOp },
    SymbolEntry { command: "supset", unicode: '\u{2283}', kind: Relation },
    SymbolEntry { command: "supseteq", unicode: '\u{2287}', kind: Relation },
    SymbolEntry { command: "surd", unicode: '\u{221A}', kind: Ordinary },
    SymbolEntry { command: "swarrow", unicode: '\u{2199}', kind: Relation },
    SymbolEntry { command: "tau", unicode: '\u{03C4}', kind: Alphabetic },
    SymbolEntry { command: "therefore", unicode: '\u{2234}', kind: Relation },
    SymbolEntry { command: "theta", unicode: '\u{03B8}', kind: Alphabetic },
    SymbolEntry { command: "times", unicode: '\u{00D7}', kind: BinaryOp },
    SymbolEntry { command: "to", unicode: '\u{2192}', kind: Relation },
    SymbolEntry { command: "top", unicode: '\u{22A4}', kind: Ordinary },
    SymbolEntry { command: "triangle", unicode: '\u{25B3}', kind: Ordinary },
    SymbolEntry { command: "triangleleft", unicode: '\u{25C1}', kind: BinaryOp },
    SymbolEntry { command: "triangleright", unicode: '\u{25B7}', kind: BinaryOp },
    SymbolEntry { command: "uparrow", unicode: '\u{2191}', kind: Relation },
    SymbolEntry { command: "updownarrow", unicode: '\u{2195}', kind: Relation },
    SymbolEntry { command: "upsilon", unicode: '\u{03C5}', kind: Alphabetic },
    SymbolEntry { command: "varepsilon", unicode: '\u{03B5}', kind: Alphabetic },
    SymbolEntry { command: "varkappa", unicode: '\u{03F0}', kind: Alphabetic },
    SymbolEntry { command: "varnothing", unicode: '\u{2205}', kind: Ordinary },
    SymbolEntry { command: "varphi", unicode: '\u{03C6}', kind: Alphabetic },
    SymbolEntry { command: "varpi", unicode: '\u{03D6}', kind: Alphabetic },
    SymbolEntry { command: "varrho", unicode: '\u{03F1}', kind: Alphabetic },
    SymbolEntry { command: "varsigma", unicode: '\u{03C2}', kind: Alphabetic },
    SymbolEntry { command: "vartheta", unicode: '\u{03D1}', kind: Alphabetic },
    SymbolEntry { command: "vdash", unicode: '\u{22A2}', kind: Relation },
    SymbolEntry { command: "vdots", unicode: '\u{22EE}', kind: Ordinary },
    SymbolEntry { command: "vee", unicode: '\u{2228}', kind: BinaryOp },
    SymbolEntry { command: "wedge", unicode: '\u{2227}', kind: BinaryOp },
    SymbolEntry { command: "wp", unicode: '\u{2118}', kind: Ordinary },
    SymbolEntry { command: "wr", unicode: '\u{2240}', kind: BinaryOp },
    SymbolEntry { command: "xi", unicode: '\u{03BE}', kind: Alphabetic },
    SymbolEntry { command: "zeta", unicode: '\u{03B6}', kind: Alphabetic },
];

/// Look up the LaTeX command for an n-ary operator Unicode character.
pub fn nary_op_to_latex(ch: char) -> Option<&'static str> {
    match ch {
        '\u{2211}' => Some("sum"),
        '\u{220F}' => Some("prod"),
        '\u{2210}' => Some("coprod"),
        '\u{222B}' => Some("int"),
        '\u{222C}' => Some("iint"),
        '\u{222D}' => Some("iiint"),
        '\u{222E}' => Some("oint"),
        '\u{22C2}' => Some("bigcap"),
        '\u{22C3}' => Some("bigcup"),
        '\u{22C0}' => Some("bigwedge"),
        '\u{22C1}' => Some("bigvee"),
        '\u{2A00}' => Some("bigodot"),
        '\u{2A01}' => Some("bigoplus"),
        '\u{2A02}' => Some("bigotimes"),
        '\u{2A04}' => Some("biguplus"),
        '\u{2A06}' => Some("bigsqcup"),
        _ => None,
    }
}

/// Check whether a LaTeX command is an n-ary (large) operator.
pub fn is_nary_command(command: &str) -> bool {
    matches!(
        command,
        "sum"
            | "prod"
            | "coprod"
            | "int"
            | "iint"
            | "iiint"
            | "oint"
            | "bigcap"
            | "bigcup"
            | "bigwedge"
            | "bigvee"
            | "bigodot"
            | "bigoplus"
            | "bigotimes"
            | "biguplus"
            | "bigsqcup"
    )
}

/// Check whether a LaTeX command is a named operator (like sin, cos, lim).
pub fn is_operator_command(command: &str) -> bool {
    matches!(
        command,
        "sin"
            | "cos"
            | "tan"
            | "sec"
            | "csc"
            | "cot"
            | "arcsin"
            | "arccos"
            | "arctan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "coth"
            | "log"
            | "ln"
            | "lg"
            | "exp"
            | "lim"
            | "liminf"
            | "limsup"
            | "sup"
            | "inf"
            | "min"
            | "max"
            | "det"
            | "dim"
            | "ker"
            | "hom"
            | "arg"
            | "deg"
            | "gcd"
            | "lcm"
            | "sgn"
            | "Pr"
            | "Var"
            | "Cov"
            | "supp"
            | "mod"
    )
}

/// Whether a named operator uses limit-style placement by default.
pub fn operator_has_limits(name: &str) -> bool {
    matches!(
        name,
        "lim" | "liminf" | "limsup" | "sup" | "inf" | "min" | "max" | "det" | "gcd" | "Pr"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_sorted() {
        // Verify the table is properly sorted for binary search
        for window in SYMBOLS.windows(2) {
            assert!(
                window[0].command < window[1].command,
                "SYMBOLS not sorted: {:?} should come before {:?}",
                window[0].command,
                window[1].command,
            );
        }
    }

    #[test]
    fn greek_lookup() {
        assert_eq!(latex_to_unicode("alpha"), Some('\u{03B1}'));
        assert_eq!(latex_to_unicode("Omega"), Some('\u{03A9}'));
        assert_eq!(latex_to_unicode("nonexistent"), None);
    }

    #[test]
    fn reverse_lookup() {
        assert_eq!(unicode_to_latex('\u{03B1}'), Some("alpha"));
        assert_eq!(unicode_to_latex('\u{2211}'), Some("sum"));
    }

    #[test]
    fn symbol_classification() {
        assert_eq!(symbol_kind('\u{2211}'), SymbolKind::LargeOp);
        assert_eq!(symbol_kind('\u{2264}'), SymbolKind::Relation);
        assert_eq!(symbol_kind('\u{00B1}'), SymbolKind::BinaryOp);
        // Unknown character defaults to Ordinary
        assert_eq!(symbol_kind('Q'), SymbolKind::Ordinary);
    }

    #[test]
    fn nary_ops() {
        assert!(is_nary_command("sum"));
        assert!(is_nary_command("int"));
        assert!(is_nary_command("bigodot"));
        assert!(is_nary_command("bigsqcup"));
        assert!(!is_nary_command("sin"));
    }

    #[test]
    fn operator_commands() {
        assert!(is_operator_command("sin"));
        assert!(is_operator_command("lim"));
        assert!(is_operator_command("lcm"));
        assert!(is_operator_command("sgn"));
        assert!(!is_operator_command("alpha"));
    }

    #[test]
    fn new_symbols_accessible() {
        // Arrows
        assert_eq!(latex_to_unicode("Longrightarrow"), Some('\u{27F9}'));
        assert_eq!(latex_to_unicode("longrightarrow"), Some('\u{27F6}'));
        assert_eq!(latex_to_unicode("updownarrow"), Some('\u{2195}'));
        assert_eq!(latex_to_unicode("nearrow"), Some('\u{2197}'));
        assert_eq!(latex_to_unicode("rightharpoonup"), Some('\u{21C0}'));

        // Logic
        assert_eq!(latex_to_unicode("therefore"), Some('\u{2234}'));
        assert_eq!(latex_to_unicode("because"), Some('\u{2235}'));
        assert_eq!(latex_to_unicode("implies"), Some('\u{27F9}'));
        assert_eq!(latex_to_unicode("iff"), Some('\u{27FA}'));
        assert_eq!(latex_to_unicode("land"), Some('\u{2227}'));
        assert_eq!(latex_to_unicode("lor"), Some('\u{2228}'));
        assert_eq!(latex_to_unicode("top"), Some('\u{22A4}'));
        assert_eq!(latex_to_unicode("bot"), Some('\u{22A5}'));

        // Relations
        assert_eq!(latex_to_unicode("leqslant"), Some('\u{2A7D}'));
        assert_eq!(latex_to_unicode("geqslant"), Some('\u{2A7E}'));
        assert_eq!(latex_to_unicode("preceq"), Some('\u{2AAF}'));
        assert_eq!(latex_to_unicode("succeq"), Some('\u{2AB0}'));
        assert_eq!(latex_to_unicode("simeq"), Some('\u{2243}'));
        assert_eq!(latex_to_unicode("mid"), Some('\u{2223}'));
        assert_eq!(latex_to_unicode("nmid"), Some('\u{2224}'));

        // Binary ops
        assert_eq!(latex_to_unicode("odot"), Some('\u{2299}'));
        assert_eq!(latex_to_unicode("oslash"), Some('\u{2298}'));
        assert_eq!(latex_to_unicode("dagger"), Some('\u{2020}'));
        assert_eq!(latex_to_unicode("amalg"), Some('\u{2A3F}'));

        // Misc
        assert_eq!(latex_to_unicode("beth"), Some('\u{2136}'));
        assert_eq!(latex_to_unicode("gimel"), Some('\u{2137}'));
        assert_eq!(latex_to_unicode("daleth"), Some('\u{2138}'));
        assert_eq!(latex_to_unicode("prime"), Some('\u{2032}'));
        assert_eq!(latex_to_unicode("square"), Some('\u{25A1}'));
        assert_eq!(latex_to_unicode("degree"), Some('\u{00B0}'));
    }

    #[test]
    fn lookup_command_binary_search() {
        let entry = lookup_command("alpha").unwrap();
        assert_eq!(entry.unicode, '\u{03B1}');
        assert_eq!(entry.kind, SymbolKind::Alphabetic);

        let entry = lookup_command("sum").unwrap();
        assert_eq!(entry.unicode, '\u{2211}');
        assert_eq!(entry.kind, SymbolKind::LargeOp);

        assert!(lookup_command("nonexistent").is_none());
    }
}
