use crate::ast::SymbolKind;

/// A symbol entry mapping a LaTeX command to a Unicode character and its math class.
#[derive(Debug, Clone, Copy)]
pub struct SymbolEntry {
    pub command: &'static str,
    pub unicode: char,
    pub kind: SymbolKind,
}

/// Look up a Unicode character by LaTeX command name (without backslash).
pub fn latex_to_unicode(command: &str) -> Option<char> {
    SYMBOLS
        .iter()
        .find(|e| e.command == command)
        .map(|e| e.unicode)
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
pub fn lookup_command(command: &str) -> Option<&'static SymbolEntry> {
    SYMBOLS.iter().find(|e| e.command == command)
}

use SymbolKind::*;

pub static SYMBOLS: &[SymbolEntry] = &[
    // === Greek lowercase ===
    SymbolEntry { command: "alpha", unicode: '\u{03B1}', kind: Alphabetic },
    SymbolEntry { command: "beta", unicode: '\u{03B2}', kind: Alphabetic },
    SymbolEntry { command: "gamma", unicode: '\u{03B3}', kind: Alphabetic },
    SymbolEntry { command: "delta", unicode: '\u{03B4}', kind: Alphabetic },
    SymbolEntry { command: "epsilon", unicode: '\u{03F5}', kind: Alphabetic },
    SymbolEntry { command: "varepsilon", unicode: '\u{03B5}', kind: Alphabetic },
    SymbolEntry { command: "zeta", unicode: '\u{03B6}', kind: Alphabetic },
    SymbolEntry { command: "eta", unicode: '\u{03B7}', kind: Alphabetic },
    SymbolEntry { command: "theta", unicode: '\u{03B8}', kind: Alphabetic },
    SymbolEntry { command: "vartheta", unicode: '\u{03D1}', kind: Alphabetic },
    SymbolEntry { command: "iota", unicode: '\u{03B9}', kind: Alphabetic },
    SymbolEntry { command: "kappa", unicode: '\u{03BA}', kind: Alphabetic },
    SymbolEntry { command: "lambda", unicode: '\u{03BB}', kind: Alphabetic },
    SymbolEntry { command: "mu", unicode: '\u{03BC}', kind: Alphabetic },
    SymbolEntry { command: "nu", unicode: '\u{03BD}', kind: Alphabetic },
    SymbolEntry { command: "xi", unicode: '\u{03BE}', kind: Alphabetic },
    SymbolEntry { command: "pi", unicode: '\u{03C0}', kind: Alphabetic },
    SymbolEntry { command: "varpi", unicode: '\u{03D6}', kind: Alphabetic },
    SymbolEntry { command: "rho", unicode: '\u{03C1}', kind: Alphabetic },
    SymbolEntry { command: "varrho", unicode: '\u{03F1}', kind: Alphabetic },
    SymbolEntry { command: "sigma", unicode: '\u{03C3}', kind: Alphabetic },
    SymbolEntry { command: "varsigma", unicode: '\u{03C2}', kind: Alphabetic },
    SymbolEntry { command: "tau", unicode: '\u{03C4}', kind: Alphabetic },
    SymbolEntry { command: "upsilon", unicode: '\u{03C5}', kind: Alphabetic },
    SymbolEntry { command: "phi", unicode: '\u{03D5}', kind: Alphabetic },
    SymbolEntry { command: "varphi", unicode: '\u{03C6}', kind: Alphabetic },
    SymbolEntry { command: "chi", unicode: '\u{03C7}', kind: Alphabetic },
    SymbolEntry { command: "psi", unicode: '\u{03C8}', kind: Alphabetic },
    SymbolEntry { command: "omega", unicode: '\u{03C9}', kind: Alphabetic },

    // === Greek uppercase ===
    SymbolEntry { command: "Gamma", unicode: '\u{0393}', kind: Alphabetic },
    SymbolEntry { command: "Delta", unicode: '\u{0394}', kind: Alphabetic },
    SymbolEntry { command: "Theta", unicode: '\u{0398}', kind: Alphabetic },
    SymbolEntry { command: "Lambda", unicode: '\u{039B}', kind: Alphabetic },
    SymbolEntry { command: "Xi", unicode: '\u{039E}', kind: Alphabetic },
    SymbolEntry { command: "Pi", unicode: '\u{03A0}', kind: Alphabetic },
    SymbolEntry { command: "Sigma", unicode: '\u{03A3}', kind: Alphabetic },
    SymbolEntry { command: "Upsilon", unicode: '\u{03A5}', kind: Alphabetic },
    SymbolEntry { command: "Phi", unicode: '\u{03A6}', kind: Alphabetic },
    SymbolEntry { command: "Psi", unicode: '\u{03A8}', kind: Alphabetic },
    SymbolEntry { command: "Omega", unicode: '\u{03A9}', kind: Alphabetic },

    // === Binary operators ===
    SymbolEntry { command: "pm", unicode: '\u{00B1}', kind: BinaryOp },
    SymbolEntry { command: "mp", unicode: '\u{2213}', kind: BinaryOp },
    SymbolEntry { command: "times", unicode: '\u{00D7}', kind: BinaryOp },
    SymbolEntry { command: "div", unicode: '\u{00F7}', kind: BinaryOp },
    SymbolEntry { command: "cdot", unicode: '\u{22C5}', kind: BinaryOp },
    SymbolEntry { command: "ast", unicode: '\u{2217}', kind: BinaryOp },
    SymbolEntry { command: "star", unicode: '\u{22C6}', kind: BinaryOp },
    SymbolEntry { command: "circ", unicode: '\u{2218}', kind: BinaryOp },
    SymbolEntry { command: "bullet", unicode: '\u{2219}', kind: BinaryOp },
    SymbolEntry { command: "oplus", unicode: '\u{2295}', kind: BinaryOp },
    SymbolEntry { command: "ominus", unicode: '\u{2296}', kind: BinaryOp },
    SymbolEntry { command: "otimes", unicode: '\u{2297}', kind: BinaryOp },
    SymbolEntry { command: "cap", unicode: '\u{2229}', kind: BinaryOp },
    SymbolEntry { command: "cup", unicode: '\u{222A}', kind: BinaryOp },
    SymbolEntry { command: "wedge", unicode: '\u{2227}', kind: BinaryOp },
    SymbolEntry { command: "vee", unicode: '\u{2228}', kind: BinaryOp },
    SymbolEntry { command: "setminus", unicode: '\u{2216}', kind: BinaryOp },

    // === Relations ===
    SymbolEntry { command: "le", unicode: '\u{2264}', kind: Relation },
    SymbolEntry { command: "leq", unicode: '\u{2264}', kind: Relation },
    SymbolEntry { command: "ge", unicode: '\u{2265}', kind: Relation },
    SymbolEntry { command: "geq", unicode: '\u{2265}', kind: Relation },
    SymbolEntry { command: "ne", unicode: '\u{2260}', kind: Relation },
    SymbolEntry { command: "neq", unicode: '\u{2260}', kind: Relation },
    SymbolEntry { command: "sim", unicode: '\u{223C}', kind: Relation },
    SymbolEntry { command: "approx", unicode: '\u{2248}', kind: Relation },
    SymbolEntry { command: "equiv", unicode: '\u{2261}', kind: Relation },
    SymbolEntry { command: "cong", unicode: '\u{2245}', kind: Relation },
    SymbolEntry { command: "propto", unicode: '\u{221D}', kind: Relation },
    SymbolEntry { command: "subset", unicode: '\u{2282}', kind: Relation },
    SymbolEntry { command: "supset", unicode: '\u{2283}', kind: Relation },
    SymbolEntry { command: "subseteq", unicode: '\u{2286}', kind: Relation },
    SymbolEntry { command: "supseteq", unicode: '\u{2287}', kind: Relation },
    SymbolEntry { command: "in", unicode: '\u{2208}', kind: Relation },
    SymbolEntry { command: "notin", unicode: '\u{2209}', kind: Relation },
    SymbolEntry { command: "ni", unicode: '\u{220B}', kind: Relation },
    SymbolEntry { command: "prec", unicode: '\u{227A}', kind: Relation },
    SymbolEntry { command: "succ", unicode: '\u{227B}', kind: Relation },
    SymbolEntry { command: "ll", unicode: '\u{226A}', kind: Relation },
    SymbolEntry { command: "gg", unicode: '\u{226B}', kind: Relation },
    SymbolEntry { command: "perp", unicode: '\u{22A5}', kind: Relation },
    SymbolEntry { command: "parallel", unicode: '\u{2225}', kind: Relation },
    SymbolEntry { command: "vdash", unicode: '\u{22A2}', kind: Relation },
    SymbolEntry { command: "dashv", unicode: '\u{22A3}', kind: Relation },
    SymbolEntry { command: "models", unicode: '\u{22A8}', kind: Relation },

    // === Arrows ===
    SymbolEntry { command: "to", unicode: '\u{2192}', kind: Relation },
    SymbolEntry { command: "rightarrow", unicode: '\u{2192}', kind: Relation },
    SymbolEntry { command: "leftarrow", unicode: '\u{2190}', kind: Relation },
    SymbolEntry { command: "leftrightarrow", unicode: '\u{2194}', kind: Relation },
    SymbolEntry { command: "Rightarrow", unicode: '\u{21D2}', kind: Relation },
    SymbolEntry { command: "Leftarrow", unicode: '\u{21D0}', kind: Relation },
    SymbolEntry { command: "Leftrightarrow", unicode: '\u{21D4}', kind: Relation },
    SymbolEntry { command: "mapsto", unicode: '\u{21A6}', kind: Relation },
    SymbolEntry { command: "hookrightarrow", unicode: '\u{21AA}', kind: Relation },
    SymbolEntry { command: "hookleftarrow", unicode: '\u{21A9}', kind: Relation },
    SymbolEntry { command: "uparrow", unicode: '\u{2191}', kind: Relation },
    SymbolEntry { command: "downarrow", unicode: '\u{2193}', kind: Relation },

    // === Large operators ===
    SymbolEntry { command: "sum", unicode: '\u{2211}', kind: LargeOp },
    SymbolEntry { command: "prod", unicode: '\u{220F}', kind: LargeOp },
    SymbolEntry { command: "coprod", unicode: '\u{2210}', kind: LargeOp },
    SymbolEntry { command: "int", unicode: '\u{222B}', kind: LargeOp },
    SymbolEntry { command: "iint", unicode: '\u{222C}', kind: LargeOp },
    SymbolEntry { command: "iiint", unicode: '\u{222D}', kind: LargeOp },
    SymbolEntry { command: "oint", unicode: '\u{222E}', kind: LargeOp },
    SymbolEntry { command: "bigcap", unicode: '\u{22C2}', kind: LargeOp },
    SymbolEntry { command: "bigcup", unicode: '\u{22C3}', kind: LargeOp },
    SymbolEntry { command: "bigwedge", unicode: '\u{22C0}', kind: LargeOp },
    SymbolEntry { command: "bigvee", unicode: '\u{22C1}', kind: LargeOp },
    SymbolEntry { command: "bigoplus", unicode: '\u{2A01}', kind: LargeOp },
    SymbolEntry { command: "bigotimes", unicode: '\u{2A02}', kind: LargeOp },

    // === Delimiters ===
    SymbolEntry { command: "langle", unicode: '\u{27E8}', kind: Open },
    SymbolEntry { command: "rangle", unicode: '\u{27E9}', kind: Close },
    SymbolEntry { command: "lfloor", unicode: '\u{230A}', kind: Open },
    SymbolEntry { command: "rfloor", unicode: '\u{230B}', kind: Close },
    SymbolEntry { command: "lceil", unicode: '\u{2308}', kind: Open },
    SymbolEntry { command: "rceil", unicode: '\u{2309}', kind: Close },
    SymbolEntry { command: "lbrace", unicode: '{', kind: Open },
    SymbolEntry { command: "rbrace", unicode: '}', kind: Close },
    SymbolEntry { command: "lvert", unicode: '|', kind: Open },
    SymbolEntry { command: "rvert", unicode: '|', kind: Close },
    SymbolEntry { command: "lVert", unicode: '\u{2016}', kind: Open },
    SymbolEntry { command: "rVert", unicode: '\u{2016}', kind: Close },
    SymbolEntry { command: "lbrack", unicode: '[', kind: Open },
    SymbolEntry { command: "rbrack", unicode: ']', kind: Close },

    // === Miscellaneous ordinary ===
    SymbolEntry { command: "infty", unicode: '\u{221E}', kind: Ordinary },
    SymbolEntry { command: "nabla", unicode: '\u{2207}', kind: Ordinary },
    SymbolEntry { command: "partial", unicode: '\u{2202}', kind: Ordinary },
    SymbolEntry { command: "forall", unicode: '\u{2200}', kind: Ordinary },
    SymbolEntry { command: "exists", unicode: '\u{2203}', kind: Ordinary },
    SymbolEntry { command: "nexists", unicode: '\u{2204}', kind: Ordinary },
    SymbolEntry { command: "emptyset", unicode: '\u{2205}', kind: Ordinary },
    SymbolEntry { command: "varnothing", unicode: '\u{2205}', kind: Ordinary },
    SymbolEntry { command: "wp", unicode: '\u{2118}', kind: Ordinary },
    SymbolEntry { command: "complement", unicode: '\u{2201}', kind: Ordinary },
    SymbolEntry { command: "neg", unicode: '\u{00AC}', kind: Ordinary },
    SymbolEntry { command: "lnot", unicode: '\u{00AC}', kind: Ordinary },
    SymbolEntry { command: "Re", unicode: '\u{211C}', kind: Ordinary },
    SymbolEntry { command: "Im", unicode: '\u{2111}', kind: Ordinary },
    SymbolEntry { command: "aleph", unicode: '\u{2135}', kind: Ordinary },
    SymbolEntry { command: "hbar", unicode: '\u{210F}', kind: Ordinary },
    SymbolEntry { command: "ell", unicode: '\u{2113}', kind: Ordinary },
    SymbolEntry { command: "imath", unicode: '\u{0131}', kind: Ordinary },
    SymbolEntry { command: "jmath", unicode: '\u{0237}', kind: Ordinary },

    // === Dots ===
    SymbolEntry { command: "ldots", unicode: '\u{2026}', kind: Ordinary },
    SymbolEntry { command: "cdots", unicode: '\u{22EF}', kind: Ordinary },
    SymbolEntry { command: "vdots", unicode: '\u{22EE}', kind: Ordinary },
    SymbolEntry { command: "ddots", unicode: '\u{22F1}', kind: Ordinary },
    SymbolEntry { command: "dots", unicode: '\u{2026}', kind: Ordinary },

    // === Punctuation ===
    SymbolEntry { command: "colon", unicode: ':', kind: Punctuation },
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
        '\u{2A01}' => Some("bigoplus"),
        '\u{2A02}' => Some("bigotimes"),
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
            | "bigoplus"
            | "bigotimes"
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
            | "Pr"
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
        assert!(!is_nary_command("sin"));
    }

    #[test]
    fn operator_commands() {
        assert!(is_operator_command("sin"));
        assert!(is_operator_command("lim"));
        assert!(!is_operator_command("alpha"));
    }
}
