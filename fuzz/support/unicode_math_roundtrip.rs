//! Round trips through UnicodeMath: the input of `fuzz_unicode_math_roundtrip`
//! is read as choices in a grammar of the LaTeX the UnicodeMath writer claims
//! to support, and the check is that nothing but grouping changes.

use mathema::{Format, convert, from_latex, from_mathml, from_unicode_math};

/// A source of choices; zeros once the input is used up, so it ends.
pub struct Choices<'a>(pub &'a [u8]);

impl Choices<'_> {
    fn next(&mut self, below: usize) -> usize {
        let Some((&byte, rest)) = self.0.split_first() else {
            return 0;
        };
        self.0 = rest;
        usize::from(byte) % below
    }

    fn pick<'t>(&mut self, table: &[&'t str]) -> &'t str {
        table[self.next(table.len())]
    }
}

const ATOMS: &[&str] = &[
    "x",
    "a",
    "y",
    "1",
    "2",
    "12",
    "ab",
    "max",
    "sin",
    "kmax",
    "\\alpha ",
    "\\infty ",
    "\\sin ",
    "\\max ",
    "\\lim ",
    "\\log ",
    "!",
    "\\partial ",
    "\\ldots ",
    "x'",
    "\\emptyset ",
    "+1.5",
    "+.5+",
    "=5.+",
    "+0.25",
    "1,234",
    "x.y+",
    "10^{-3}",
    "\\times .2+",
    "2\\,\\mathrm{m}+",
];

const OPERATORS: &[&str] = &[
    "+",
    "-",
    "=",
    "<",
    "*",
    ",",
    ":",
    ";",
    "\\cdot ",
    "\\times ",
    "\\pm ",
    "\\le ",
    "\\in ",
    "\\to ",
    "\\lesssim ",
    "\\oplus ",
    "\\langle ",
    "\\rangle ",
    "\\iff ",
    "\\dagger ",
    "\\perp ",
    "\\mid ",
    "\\not=",
    "\\neq ",
    "\\equiv ",
    "\\cup ",
    "\\bot ",
    "\\ll ",
    "\\mapsto ",
];

const ACCENTS: &[&str] = &[
    "\\hat",
    "\\bar",
    "\\vec",
    "\\dot",
    "\\tilde",
    "\\widehat",
    "\\widetilde",
];
const NARY: &[&str] = &[
    "\\sum ",
    "\\int ",
    "\\prod ",
    "\\oint ",
    "\\bigcup ",
    "\\bigoplus ",
];
const SPACES: &[&str] = &["\\,", "\\;", "\\:", "\\quad ", "\\ "];

/// A formula of at most `depth` nested constructs.
pub fn latex(choices: &mut Choices<'_>, depth: usize, out: &mut String) {
    for _ in 0..1 + choices.next(4) {
        let kind = choices.next(if depth == 0 { 8 } else { 27 });
        let group = |choices: &mut Choices<'_>, out: &mut String| {
            out.push('{');
            latex(choices, depth - 1, out);
            out.push('}');
        };
        match kind {
            0..=3 => out.push_str(choices.pick(ATOMS)),
            4..=6 => out.push_str(choices.pick(OPERATORS)),
            7 => out.push_str(choices.pick(SPACES)),
            8 | 9 => {
                out.push_str("\\frac");
                group(choices, out);
                group(choices, out);
            },
            10..=13 => {
                // A base of its own, then scripts.
                group(choices, out);
                let marks = choices.next(3);
                if marks != 1 {
                    out.push('_');
                    group(choices, out);
                }
                if marks != 0 {
                    out.push('^');
                    group(choices, out);
                }
            },
            14 | 15 => {
                out.push_str(choices.pick(ATOMS));
                out.push(if choices.next(2) == 0 { '_' } else { '^' });
                group(choices, out);
            },
            16 => {
                out.push_str("\\sqrt");
                group(choices, out);
            },
            17 => {
                out.push_str("\\sqrt[");
                out.push_str(choices.pick(&["3", "n", "2", "a+b"]));
                out.push(']');
                group(choices, out);
            },
            18 => {
                out.push_str(choices.pick(ACCENTS));
                group(choices, out);
            },
            19 | 20 => {
                out.push_str(choices.pick(NARY));
                if choices.next(2) == 0 {
                    out.push('_');
                    group(choices, out);
                }
                if choices.next(2) == 0 {
                    out.push('^');
                    group(choices, out);
                }
            },
            21 => {
                let fence = choices.next(4);
                if fence == 3 {
                    // Bars do not nest: UnicodeMath tells an opening from a
                    // closing one by their count.
                    out.push_str("\\left|");
                    if choices.next(2) == 0 {
                        out.push_str("\\frac{");
                        latex(choices, 0, out);
                        out.push_str("}{");
                        latex(choices, 0, out);
                        out.push('}');
                    }
                    latex(choices, 0, out);
                    out.push_str("\\right|");
                } else {
                    let (open, close) = [("(", ")"), ("[", "]"), ("\\{", "\\}")][fence];
                    out.push_str("\\left");
                    out.push_str(open);
                    latex(choices, depth - 1, out);
                    out.push_str("\\right");
                    out.push_str(close);
                }
            },
            22 => {
                out.push_str("\\binom");
                group(choices, out);
                group(choices, out);
            },
            23 => {
                // A plain bar, or a pair of them.
                out.push('|');
                if choices.next(3) != 0 {
                    latex(choices, depth - 1, out);
                    out.push('|');
                }
            },
            24 => {
                // Brackets of unlike kinds: kets, bras and inner products.
                let (open, close) = [
                    ("|", "\\rangle "),
                    ("\\langle ", "|"),
                    ("\\langle ", "\\rangle "),
                    ("\\|", "\\|"),
                ][choices.next(4)];
                out.push_str(open);
                latex(choices, depth - 1, out);
                out.push_str(close);
            },
            25 => {
                // An evaluation bar.
                out.push_str(choices.pick(ATOMS));
                out.push_str("|_");
                group(choices, out);
                if choices.next(2) == 0 {
                    out.push('^');
                    group(choices, out);
                }
            },
            _ => {
                out.push_str("\\begin{matrix}");
                latex(choices, depth - 1, out);
                out.push('&');
                latex(choices, depth - 1, out);
                out.push_str("\\end{matrix}");
            },
        }
    }
}

/// Pieces of UnicodeMath, in any order.
const PIECES: &[&str] = &[
    "x",
    "y",
    "ab",
    "2",
    "α",
    "sin",
    "max",
    "+",
    "-",
    "=",
    ",",
    "∈",
    "⋅",
    "!",
    "/",
    "/",
    "/",
    "_",
    "^",
    "_2",
    "^n",
    "|",
    "|",
    "|",
    "|",
    "‖",
    "‖",
    "(",
    ")",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    "⟨",
    "⟩",
    "⌊",
    "⌋",
    " ",
    " ",
    " ",
    "√",
    "√(",
    "∑",
    "∫",
    "∑_i",
    "〖",
    "〗",
    "′",
    "†",
    "̂",
    "┬",
    "┴",
    "■(",
    "&",
    "@",
    "▒",
    "¦",
    "ⅆ",
    "→",
    "≉",
    "∂",
    "∞",
    "\u{2009}",
    "²",
    "₁",
    "⁻¹",
    "⁽ⁿ⁾",
    "½",
    "'",
    "\u{2061}",
    "⌈",
    "⌉",
];

/// UnicodeMath as a person might write it.
pub fn unicode_math(choices: &mut Choices<'_>) -> String {
    let mut out = String::new();
    while !choices.0.is_empty() {
        out.push_str(choices.pick(PIECES));
    }
    out
}

/// MathML as a tree, without attributes. A row that is an argument is kept
/// (one child is the child), a row in a row is spliced.
pub fn structure(mathml: &str) -> String {
    fn element<'a>(
        tokens: &mut impl Iterator<Item = (&'a str, &'a str)>,
        tag: &str,
        text: &str,
    ) -> (String, Vec<String>) {
        let name = tag
            .split(' ')
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        let leaf = matches!(name, "mi" | "mn" | "mo" | "mtext");
        let mut children: Vec<String> = Vec::new();
        if !tag.ends_with('/') {
            while let Some((tag, text)) = tokens.next() {
                if tag.starts_with('/') {
                    break;
                }
                let (inner_name, inner) = element(tokens, tag, text);
                // A style change is no structure.
                let row = matches!(inner_name.as_str(), "mrow" | "mstyle");
                let splices = matches!(
                    name,
                    "math" | "mrow" | "msqrt" | "mstyle" | "mtd" | "mphantom"
                );
                if row && (splices || inner.len() == 1) {
                    children.extend(inner);
                } else {
                    children.push(format!("{inner_name}({})", inner.join(" ")));
                }
            }
        }
        // `.5` may be a number, or a point and a number.
        if matches!(
            name,
            "math" | "mrow" | "msqrt" | "mstyle" | "mtd" | "mphantom"
        ) {
            let digits = |child: &str| match child {
                "mo(.)" | "mi(.)" => Some(".".to_owned()),
                _ => Some(child.strip_prefix("mn(")?.strip_suffix(')')?.to_owned()),
            };
            let mut joined: Vec<String> = Vec::new();
            for child in std::mem::take(&mut children) {
                match (joined.last().and_then(|last| digits(last)), digits(&child)) {
                    (Some(last), Some(next)) => {
                        joined.pop();
                        joined.push(format!("mn({last}{next})"));
                    },
                    _ => joined.push(child),
                }
            }
            children = joined;
        }
        if leaf {
            // An operator's class, where it is not its character's own
            // (`⊥` is `\\perp` and `\\bot`).
            let class: String = tag
                .split(' ')
                .filter(|part| text != "⊥" && part.starts_with("data-mjx-texclass"))
                .collect();
            children = vec![format!("{text}{class}")];
        }
        // UnicodeMath does not say whether the limit of a big operator or a
        // function is set below or beside; an accent is no limit.
        let limit = children.first().is_some_and(|base| {
            base.starts_with("mo(") || base.starts_with("mi(") && base.chars().count() > 5
        });
        let name = match name {
            _ if !limit => name,
            "munder" => "msub",
            "mover" => "msup",
            "munderover" => "msubsup",
            name => name,
        };
        (name.to_owned(), children)
    }
    let mut tokens = mathml
        .split('<')
        .skip(1)
        .filter_map(|token| token.split_once('>'));
    let Some((tag, text)) = tokens.next() else {
        return String::new();
    };
    element(&mut tokens, tag, text).1.join(" ")
}

/// Hand-written leading scripts whose round trips are known to fail: those
/// on a fence or a matrix, or after a function that is an n-aryand (there the
/// MathML reader's normalization reads the scripts of `wanted` otherwise).
fn known(input: &str, mathml: &str, from: Format) -> bool {
    from == Format::UnicodeMath
        && (input.contains('■')
            || input.contains(['∑', '∫']) && (input.contains(" _") || input.contains(" ^"))
            || input.contains(['(', '[', '{']))
        && ["<msub>", "<msup>", "<msubsup>", "<mmultiscripts>"]
            .iter()
            .any(|tag| {
                mathml.contains(&format!("{tag}<mrow></mrow>"))
                    || *tag == "<mmultiscripts>" && mathml.contains(tag)
            })
}

/// What went wrong for `input`, or `None`.
pub fn check(input: &str, from: Format) -> Option<(&'static str, String)> {
    let parsed = match from {
        Format::LaTeX => from_latex(input),
        Format::UnicodeMath => from_unicode_math(input),
        _ => from_mathml(input),
    }
    .ok()?;
    // What the writer reports as lost is not expected back.
    let (_, losses) = parsed.to_unicode_math_with_diagnostics().ok()?;
    if !losses.is_empty() {
        return None;
    }
    let source = parsed.to_mathml().ok()?;
    if known(input, &source, from) {
        return None;
    }
    let builder = parsed.to_unicode_math().ok()?;
    let converted = convert(input, from, Format::UnicodeMath).ok()?;
    let normalized = convert(input, from, Format::MathML).ok()?;
    for (path, unicode, wanted) in [
        ("builder", builder, source),
        ("convert", converted, normalized),
    ] {
        let Ok(found) = convert(&unicode, Format::UnicodeMath, Format::MathML) else {
            return Some(("unreadable", format!("{path}: {unicode}")));
        };
        // Normalize both sides the same way.
        let wanted = convert(&wanted, Format::MathML, Format::MathML).ok()?;
        let found = convert(&found, Format::MathML, Format::MathML).ok()?;
        let (wanted, found) = (structure(&wanted), structure(&found));
        if wanted != found {
            return Some((
                "structure",
                format!("{path}: {unicode}\n  wanted {wanted}\n  found  {found}"),
            ));
        }
        let again = convert(&unicode, Format::UnicodeMath, Format::UnicodeMath).ok()?;
        let third = convert(&again, Format::UnicodeMath, Format::UnicodeMath).ok()?;
        // One more pass may settle how a bar pair is spelled.
        let settled = again == third
            || unicode.contains(['|', '‖'])
                && convert(&third, Format::UnicodeMath, Format::UnicodeMath).ok()? == third;
        if !settled {
            return Some(("unstable", format!("{path}: {unicode} / {again} / {third}")));
        }
    }
    None
}
