//! `physics` package: variadic derivative operators.
//!
//! Most `physics` commands have a fixed arity and are provided as preprocessor
//! macros in [`crate::latex::packages::physics`]. The derivative operators
//! (`\dv`, `\pdv`, `\fdv`) are genuinely *variadic* — they accept an optional
//! `[order]`, an optional `*` (slashed/inline form), and a variable number of
//! brace groups (operator form `\dv{x}`, derivative form `\dv{f}{x}`, and the
//! mixed partial form `\pdv{f}{x}{y}`). A fixed-arity macro body cannot express
//! that, so — like [`super::siunitx`] and [`super::mhchem`] — these are
//! **translated** to standard LaTeX (a `\frac`/slash over `\mathrm{d}` /
//! `\partial` / `\delta`) and re-parsed through the normal pipeline.
//!
//! Faithful to `physics.sty`:
//!
//! * `\dv{x}`          → `\frac{\mathrm{d}}{\mathrm{d}x}`        (operator form)
//! * `\dv{f}{x}`       → `\frac{\mathrm{d}f}{\mathrm{d}x}`
//! * `\dv[n]{f}{x}`    → `\frac{\mathrm{d}^{n}f}{\mathrm{d}x^{n}}`
//! * `\dv*{f}{x}`      → `\mathrm{d}f/\mathrm{d}x`               (slashed form)
//! * `\pdv{f}{x}{y}`   → `\frac{\partial^{2}f}{\partial x\partial y}`
//! * `\fdv{F}{g}`      → `\frac{\delta F}{\delta g}`             (functional)

use super::mhchem::capture_braced;

/// Maximum number of consecutive brace groups consumed by a derivative command.
/// `\pdv` mixed partials want several denominators; `\dv`/`\fdv` cap at two.
const MAX_PDV_ARGS: usize = 6;

/// Translate a `physics` derivative command (`name` is `dv`, `pdv`, or `fdv`;
/// `input` is positioned right after the command name) into standard LaTeX.
///
/// Returns `(remaining_input, latex)`. Always succeeds; malformed input
/// degrades to a best-effort string and never panics.
#[must_use]
pub fn translate_derivative<'a>(name: &str, input: &'a str) -> (&'a str, String) {
    // The differential core glyph for this operator.
    let core = match name {
        "pdv" => "\\partial",
        "fdv" => "\\delta",
        _ => "\\mathrm{d}",
    };
    let max_args = if name == "pdv" { MAX_PDV_ARGS } else { 2 };

    // Parse a leading `*` (slashed form) and/or `[order]` in any order.
    let mut rest = input.trim_start();
    let mut star = false;
    let mut order: Option<String> = None;
    loop {
        let trimmed = rest.trim_start();
        if let Some(r) = trimmed.strip_prefix('*') {
            star = true;
            rest = r;
            continue;
        }
        if trimmed.starts_with('[')
            && let Some((r, ord)) = capture_bracket(trimmed)
        {
            let ord = ord.trim();
            if !ord.is_empty() {
                order = Some(ord.to_owned());
            }
            rest = r;
            continue;
        }
        break;
    }

    // Collect the brace-group arguments.
    let mut args: Vec<String> = Vec::new();
    while args.len() < max_args {
        match capture_braced(rest) {
            Some((r, body)) => {
                args.push(body.to_owned());
                rest = r;
            },
            None => break,
        }
    }

    let latex = build(core, star, order.as_deref(), &args);
    (rest, latex)
}

/// Build the `\frac{...}{...}` (or slashed) derivative body.
fn build(core: &str, star: bool, order: Option<&str>, args: &[String]) -> String {
    // Helper: `core` with an optional `^{order}` superscript (suppressed for 1).
    let with_sup = |ord: &str| -> String {
        if ord == "1" || ord.is_empty() {
            core.to_owned()
        } else {
            format!("{core}^{{{ord}}}")
        }
    };
    // Helper: append a trailing token with a separating space when non-empty.
    let tail = |head: String, t: &str| -> String {
        if t.is_empty() {
            head
        } else {
            format!("{head} {t}")
        }
    };

    let (num, den) = match args.len() {
        0 => (with_sup("1"), core.to_owned()),
        1 => {
            // Operator form: \dv{x} → d / dx.
            let o = order.unwrap_or("1");
            let num = with_sup(o);
            let den = tail(with_sup(o), &args[0]);
            (num, den)
        },
        _ => {
            // Derivative form: f over one-or-more variables.
            let f = &args[0];
            let vars = &args[1..];
            let total = match order {
                Some(o) => o.to_owned(),
                None if vars.len() > 1 => vars.len().to_string(),
                None => "1".to_owned(),
            };
            let num = tail(with_sup(&total), f);
            let den = if vars.len() == 1 {
                // Single variable: honour an explicit [order] on the variable.
                let o = order.unwrap_or("1");
                let v = tail(with_sup("1"), &vars[0]);
                if o == "1" { v } else { format!("{v}^{{{o}}}") }
            } else {
                vars.iter()
                    .map(|v| tail(with_sup("1"), v))
                    .collect::<Vec<_>>()
                    .join("")
            };
            (num, den)
        },
    };

    if star {
        format!("{num}/{den}")
    } else {
        format!("\\frac{{{num}}}{{{den}}}")
    }
}

/// Capture one `[ ... ]` group, returning `(rest, inner)`.
fn capture_bracket(input: &str) -> Option<(&str, &str)> {
    let rest = input.trim_start().strip_prefix('[')?;
    let mut depth = 1usize;
    for (i, &b) in rest.as_bytes().iter().enumerate() {
        match b {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&rest[i + 1..], &rest[..i]));
                }
            },
            _ => {},
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(name: &str, input: &str) -> String {
        translate_derivative(name, input).1
    }

    #[test]
    fn dv_operator_form() {
        assert_eq!(tr("dv", "{x}"), "\\frac{\\mathrm{d}}{\\mathrm{d} x}");
    }

    #[test]
    fn dv_two_arg() {
        assert_eq!(tr("dv", "{f}{x}"), "\\frac{\\mathrm{d} f}{\\mathrm{d} x}");
    }

    #[test]
    fn dv_order() {
        assert_eq!(
            tr("dv", "[n]{f}{x}"),
            "\\frac{\\mathrm{d}^{n} f}{\\mathrm{d} x^{n}}"
        );
    }

    #[test]
    fn dv_star_slash() {
        assert_eq!(tr("dv", "*{f}{x}"), "\\mathrm{d} f/\\mathrm{d} x");
    }

    #[test]
    fn pdv_mixed_partial() {
        assert_eq!(
            tr("pdv", "{f}{x}{y}"),
            "\\frac{\\partial^{2} f}{\\partial x\\partial y}"
        );
    }

    #[test]
    fn pdv_two_arg() {
        assert_eq!(tr("pdv", "{f}{x}"), "\\frac{\\partial f}{\\partial x}");
    }

    #[test]
    fn fdv_functional() {
        assert_eq!(tr("fdv", "{F}{g}"), "\\frac{\\delta F}{\\delta g}");
    }

    #[test]
    fn leftover_after_args_is_returned() {
        let (rest, _) = translate_derivative("dv", "{f}{x} + 1");
        assert_eq!(rest, " + 1");
    }
}
