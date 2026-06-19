#[cfg(feature = "render")]
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use mathema::ast::{DisplayStyle, MathNode};
use mathema::error::{Config, Strictness};
use mathema::from_latex_with;

/// A formula entry from formulas.json.
struct Formula {
    name: String,
    tex: String,
    display: bool,
}

fn parse_formulas(data: &str) -> Vec<Formula> {
    let arr: Vec<serde_json::Value> = serde_json::from_str(data).unwrap();
    arr.into_iter()
        .map(|v| Formula {
            name: v["name"].as_str().unwrap_or("unknown").to_string(),
            tex: v["tex"].as_str().unwrap_or("").to_string(),
            display: v["display"].as_bool().unwrap_or(false),
        })
        .collect()
}

/// Recursively walk an AST and collect multi-character identifiers
/// that look like they might be unhandled LaTeX commands.
fn collect_suspicious_identifiers(node: &MathNode, found: &mut Vec<String>) {
    match node {
        MathNode::Identifier(s) => {
            // Multi-character identifiers that are all-ASCII-alphabetic
            // and appear in the TeX source preceded by backslash
            // are likely unhandled commands rendered as literal text.
            if s.len() > 1
                && s.chars().all(|c| c.is_ascii_alphabetic())
                && !is_known_math_function(s)
            {
                found.push(s.clone());
            }
        },
        MathNode::Row(children) | MathNode::Styled(_, children) => {
            for child in children {
                collect_suspicious_identifiers(child, found);
            }
        },
        MathNode::Fenced { content, .. } => {
            for item in content {
                if let mathema::ast::FencedItem::Element(n) = item {
                    collect_suspicious_identifiers(n, found);
                }
            }
        },
        MathNode::Subscript { base, sub } => {
            collect_suspicious_identifiers(base, found);
            collect_suspicious_identifiers(sub, found);
        },
        MathNode::Superscript { base, sup } => {
            collect_suspicious_identifiers(base, found);
            collect_suspicious_identifiers(sup, found);
        },
        MathNode::SubSup { base, sub, sup } => {
            collect_suspicious_identifiers(base, found);
            collect_suspicious_identifiers(sub, found);
            collect_suspicious_identifiers(sup, found);
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            collect_suspicious_identifiers(base, found);
            if let Some(s) = pre_sub {
                collect_suspicious_identifiers(s, found);
            }
            if let Some(s) = pre_sup {
                collect_suspicious_identifiers(s, found);
            }
        },
        MathNode::Overscript { base, over, .. } => {
            collect_suspicious_identifiers(base, found);
            collect_suspicious_identifiers(over, found);
        },
        MathNode::Underscript { base, under, .. } => {
            collect_suspicious_identifiers(base, found);
            collect_suspicious_identifiers(under, found);
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            collect_suspicious_identifiers(base, found);
            collect_suspicious_identifiers(under, found);
            collect_suspicious_identifiers(over, found);
        },
        MathNode::Fraction { num, den, .. } => {
            collect_suspicious_identifiers(num, found);
            collect_suspicious_identifiers(den, found);
        },
        MathNode::Sqrt(inner) => {
            collect_suspicious_identifiers(inner, found);
        },
        MathNode::Root { index, radicand } => {
            collect_suspicious_identifiers(index, found);
            collect_suspicious_identifiers(radicand, found);
        },
        MathNode::Nary { sub, sup, body, .. } => {
            if let Some(s) = sub {
                collect_suspicious_identifiers(s, found);
            }
            if let Some(s) = sup {
                collect_suspicious_identifiers(s, found);
            }
            collect_suspicious_identifiers(body, found);
        },
        MathNode::Accent { base, .. } => {
            collect_suspicious_identifiers(base, found);
        },
        MathNode::Array { rows, .. } => {
            for row in rows {
                for cell in row {
                    for n in cell {
                        collect_suspicious_identifiers(n, found);
                    }
                }
            }
        },
        MathNode::Phantom(inner, _) | MathNode::Boxed { content: inner, .. } => {
            collect_suspicious_identifiers(inner, found);
        },
        MathNode::Cancel { content, .. }
        | MathNode::Scaled { content, .. }
        | MathNode::RaiseBox { content, .. } => {
            collect_suspicious_identifiers(content, found);
        },
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => {
            collect_suspicious_identifiers(display, found);
            collect_suspicious_identifiers(text, found);
            collect_suspicious_identifiers(script, found);
            collect_suspicious_identifiers(script_script, found);
        },
        MathNode::MathAtom { content, .. } => {
            collect_suspicious_identifiers(content, found);
        },
        // Leaves that don't contain sub-nodes
        MathNode::Number(_)
        | MathNode::Operator(_)
        | MathNode::Symbol(_, _)
        | MathNode::Text(_, _)
        | MathNode::Space(_)
        | MathNode::Rule { .. } => {},
        // `MathNode` is `#[non_exhaustive]`; ignore any future leaf variants.
        _ => {},
    }
}

/// Check if a command appears in the TeX source with a backslash prefix.
fn is_backslash_command_in_tex(tex: &str, name: &str) -> bool {
    let pattern = format!("\\{name}");
    // Check for the pattern followed by a non-alphabetic character or end of string
    let mut search_from = 0;
    while let Some(pos) = tex[search_from..].find(&pattern) {
        let abs_pos = search_from + pos;
        let after = abs_pos + pattern.len();
        if after >= tex.len() {
            return true; // at end of string
        }
        let next_char = tex[after..].chars().next().unwrap();
        if !next_char.is_ascii_alphabetic() {
            return true; // followed by non-alpha = a real command
        }
        search_from = abs_pos + 1;
    }
    false
}

/// Known math function names and common multi-char identifiers that legitimately
/// appear as multi-character Identifier nodes (NOT unhandled commands).
fn is_known_math_function(s: &str) -> bool {
    matches!(
        s,
        // Standard math operators (these should be Operator nodes, but some
        // might legitimately end up as Identifiers in certain contexts)
        "sin" | "cos" | "tan" | "cot" | "sec" | "csc"
        | "arcsin" | "arccos" | "arctan" | "arccot" | "arcsec" | "arccsc"
        | "sinh" | "cosh" | "tanh" | "coth" | "sech" | "csch"
        | "log" | "ln" | "exp" | "lg" | "lim" | "liminf" | "limsup"
        | "min" | "max" | "inf" | "sup" | "dim" | "det" | "gcd" | "lcm"
        | "ker" | "hom" | "arg" | "deg"
        | "Pr" | "Re" | "Im"
        | "supp" | "span" | "rank" | "sign" | "sgn"
        | "proj" | "injlim" | "projlim" | "varinjlim" | "varprojlim"
        | "mod" | "bmod" | "pmod"
        // Common multi-letter variable names from physics/math
        // (these are real identifiers, not commands)
        | "dx" | "dy" | "dz" | "dt" | "dr"
        | "if" | "or" | "xor" | "and" | "not"
        // Standard TeX font-changed identifiers
        | "Tr" | "tr" | "diag" | "GL" | "SL" | "Hom" | "End" | "Aut"
        | "Ext" | "Tor" | "Spec" | "Ann" | "Gal" | "Frac" | "char"
        | "ord" | "card" | "coker" | "coim" | "codim" | "null" | "range"
        | "vol" | "curl" | "grad" | "dist" | "diam" | "conv"
        | "sgr" | "spl" | "spn"
    )
}

/// SVG text content extraction (for double checking)
#[allow(dead_code)]
fn extract_svg_text_content(svg: &str) -> Vec<String> {
    let mut texts = Vec::new();
    let mut search_from = 0;
    while let Some(start_tag) = svg[search_from..].find("<text") {
        let abs_start = search_from + start_tag;
        if let Some(tag_close) = svg[abs_start..].find('>') {
            let content_start = abs_start + tag_close + 1;
            if let Some(end_tag) = svg[content_start..].find("</text>") {
                let content_end = content_start + end_tag;
                let content = &svg[content_start..content_end];
                texts.push(content.to_string());
                search_from = content_end + 7;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    texts
}

#[cfg(feature = "render")]
#[test]
fn find_unsupported_commands() {
    let data =
        std::fs::read_to_string("tests/data/formulas.json").expect("Failed to read formulas.json");
    let formulas = parse_formulas(&data);

    let total = formulas.len();
    println!("\n=== Unsupported Command Detection (AST-based) ===");
    println!("Total formulas: {total}");

    let mut parse_failures: Vec<(String, String)> = Vec::new();
    let mut render_failures: Vec<(String, String)> = Vec::new();
    // command_name -> list of (formula_name, tex_snippet)
    let mut literal_commands: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut formulas_with_issues: BTreeSet<String> = BTreeSet::new();
    let mut success_count = 0;

    for formula in &formulas {
        let config = Config::default()
            .with_strictness(Strictness::Lenient)
            .with_display(if formula.display {
                DisplayStyle::Block
            } else {
                DisplayStyle::Inline
            });

        let result = from_latex_with(&formula.tex, &config);
        match result {
            Err(e) => {
                parse_failures.push((formula.name.clone(), format!("{e}")));
                continue;
            },
            Ok(convert_result) => {
                // First check: AST inspection for multi-char Identifier nodes
                let expr = convert_result.expr();
                let mut suspicious = Vec::new();
                for node in &expr.nodes {
                    collect_suspicious_identifiers(node, &mut suspicious);
                }

                // Filter: only count those that appear in the TeX source as \command
                let confirmed: Vec<String> = suspicious
                    .into_iter()
                    .filter(|name| is_backslash_command_in_tex(&formula.tex, name))
                    .collect();

                if !confirmed.is_empty() {
                    for cmd in &confirmed {
                        literal_commands
                            .entry(cmd.clone())
                            .or_default()
                            .push((formula.name.clone(), formula.tex.chars().take(80).collect()));
                        formulas_with_issues.insert(formula.name.clone());
                    }
                }

                // Second check: try rendering to SVG
                match convert_result.to_svg() {
                    Err(e) => {
                        render_failures.push((formula.name.clone(), format!("{e}")));
                    },
                    Ok(_svg) => {
                        success_count += 1;
                    },
                }
            },
        }
    }

    // Report results
    println!("\n--- Parse Failures ({}) ---", parse_failures.len());
    for (name, err) in &parse_failures {
        println!("  {name}: {err}");
    }

    println!("\n--- Render Failures ({}) ---", render_failures.len());
    for (name, err) in &render_failures {
        println!("  {name}: {err}");
    }

    println!("\n--- Successfully Rendered: {success_count}/{total} ---");

    println!(
        "\n--- Unsupported Commands as Literal Identifiers ({} unique commands, {} formulas affected) ---",
        literal_commands.len(),
        formulas_with_issues.len()
    );

    // Sort by number of affected formulas (descending) for the most impactful issues first
    let mut sorted_commands: Vec<(String, Vec<(String, String)>)> =
        literal_commands.into_iter().collect();
    sorted_commands.sort_by_key(|c| std::cmp::Reverse(c.1.len()));

    for (cmd, entries) in &sorted_commands {
        println!("\n  \\{cmd} ({} formulas):", entries.len());
        for (name, tex) in entries {
            println!("    - {name}: {tex}");
        }
    }

    // Final summary
    println!("\n\n========== SUMMARY ==========");
    println!("Total formulas tested:                 {total}");
    println!(
        "Parse failures:                        {}",
        parse_failures.len()
    );
    println!(
        "Render failures:                       {}",
        render_failures.len()
    );
    println!("Successfully rendered to SVG:          {success_count}");
    println!(
        "Formulas with literal command text:     {}",
        formulas_with_issues.len()
    );
    println!(
        "Unique unsupported commands found:      {}",
        sorted_commands.len()
    );
    println!("================================");
}

/// Additional test: verify specific known-problematic commands
#[cfg(feature = "render")]
#[test]
fn verify_specific_commands() {
    println!("\n=== Individual Command Verification ===");

    let test_cases = vec![
        ("\\cfrac{a}{b}", "cfrac"),
        ("\\over", "over (infix)"),
        ("\\atop", "atop (infix)"),
        ("\\above 1pt", "above (infix)"),
        ("\\choose", "choose (infix)"),
        ("\\ensuremath{x}", "ensuremath"),
        ("\\dddot{x}", "dddot"),
        ("\\ddddot{x}", "ddddot"),
        ("\\mathring{x}", "mathring"),
        ("\\overgroup{x}", "overgroup"),
        ("\\undergroup{x}", "undergroup"),
        ("\\overlinesegment{x}", "overlinesegment"),
        ("\\underlinesegment{x}", "underlinesegment"),
        ("\\sout{text}", "sout"),
        ("\\hspace{1em}", "hspace"),
        ("\\rule{1em}{2pt}", "rule"),
        ("\\raisebox{1em}{x}", "raisebox"),
        ("\\centernot{=}", "centernot"),
        ("\\href{url}{text}", "href"),
        ("\\url{example.com}", "url"),
        ("\\includegraphics{img}", "includegraphics"),
        ("\\xleftharpoondown[a]{b}", "xleftharpoondown"),
        ("\\xleftharpoonup[a]{b}", "xleftharpoonup"),
        ("\\xrightharpoondown[a]{b}", "xrightharpoondown"),
        ("\\xrightharpoonup[a]{b}", "xrightharpoonup"),
        ("\\xlongequal[a]{b}", "xlongequal"),
        ("\\DeclareMathOperator{\\Tr}{Tr}", "DeclareMathOperator"),
        ("\\idotsint", "idotsint"),
        ("\\doteq", "doteq"),
        ("\\pmb{x}", "pmb"),
        ("\\colorbox{red}{x}", "colorbox"),
        ("\\fcolorbox{red}{blue}{x}", "fcolorbox"),
        ("\\KaTeX", "KaTeX"),
        ("\\LaTeX", "LaTeX"),
        ("\\TeX", "TeX"),
        ("\\htmlId{id}{x}", "htmlId"),
        ("\\htmlClass{cls}{x}", "htmlClass"),
        ("\\lvert x \\rvert", "lvert/rvert"),
        ("\\lVert x \\rVert", "lVert/rVert"),
        ("\\middle|", "middle"),
        ("\\hline", "hline"),
        ("\\hdashline", "hdashline"),
        ("\\tiny x", "tiny"),
        ("\\small x", "small"),
        ("\\normalsize x", "normalsize"),
        ("\\large x", "large"),
        ("\\Large x", "Large"),
        ("\\LARGE x", "LARGE"),
        ("\\huge x", "huge"),
        ("\\Huge x", "Huge"),
        ("\\textnormal{x}", "textnormal"),
        ("\\textup{x}", "textup"),
    ];

    let config = Config::default()
        .with_strictness(Strictness::Lenient)
        .with_display(DisplayStyle::Inline);

    let mut unsupported = Vec::new();
    let mut supported = Vec::new();
    let mut parse_error = Vec::new();

    for (tex, label) in &test_cases {
        match from_latex_with(tex, &config) {
            Err(e) => {
                parse_error.push((*label, format!("{e}")));
            },
            Ok(result) => {
                let expr = result.expr();
                let mut suspicious = Vec::new();
                for node in &expr.nodes {
                    collect_suspicious_identifiers(node, &mut suspicious);
                }

                // Check if any of the suspicious identifiers correspond to commands in the input
                let has_literal = suspicious
                    .iter()
                    .any(|name| is_backslash_command_in_tex(tex, name));

                if has_literal {
                    unsupported.push((*label, *tex, suspicious));
                } else {
                    supported.push((*label, *tex));
                }
            },
        }
    }

    println!("\nSupported commands ({}):", supported.len());
    for (label, tex) in &supported {
        println!("  [OK] {label}: {tex}");
    }

    println!(
        "\nUnsupported (rendered as literal text) ({}):",
        unsupported.len()
    );
    for (label, tex, ids) in &unsupported {
        println!("  [FAIL] {label}: {tex}  => identifiers: {:?}", ids);
    }

    println!("\nParse errors ({}):", parse_error.len());
    for (label, err) in &parse_error {
        println!("  [ERROR] {label}: {err}");
    }
}

/// Collect the set of LaTeX commands in `path` that the parser lowers to literal
/// multi-character identifiers (i.e. it does not understand the command and
/// emits its name as text). Returns `(unsupported_commands, parse_failures)`.
///
/// This does not require the `render` feature, so it runs under the default
/// `cargo test` and can act as a regression gate over the full corpus.
fn unsupported_commands_in(path: &str) -> (BTreeSet<String>, Vec<(String, String)>) {
    let data = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let formulas = parse_formulas(&data);

    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut parse_failures: Vec<(String, String)> = Vec::new();

    for formula in &formulas {
        let config = Config::default()
            .with_strictness(Strictness::Lenient)
            .with_display(if formula.display {
                DisplayStyle::Block
            } else {
                DisplayStyle::Inline
            });
        match from_latex_with(&formula.tex, &config) {
            Err(e) => parse_failures.push((formula.name.clone(), e.to_string())),
            Ok(result) => {
                let expr = result.expr();
                let mut suspicious = Vec::new();
                for node in &expr.nodes {
                    collect_suspicious_identifiers(node, &mut suspicious);
                }
                for name in suspicious {
                    if is_backslash_command_in_tex(&formula.tex, &name) {
                        found.insert(name);
                    }
                }
            },
        }
    }
    (found, parse_failures)
}

/// Regression gate over the 126-formula KaTeX corpus.
///
/// Unlike the report-only `find_unsupported_commands` test, this one *asserts*:
/// lenient parsing must never hard-error on the curated corpus, and no LaTeX
/// command outside the recorded baseline may be silently lowered to literal
/// text. When a command gains real support, remove it from `KNOWN_UNSUPPORTED`;
/// the test then guards against regressing it. The baseline must only shrink.
#[test]
fn katex_corpus_no_new_unsupported_commands() {
    // Commands the corpus currently renders as literal identifiers. Keep sorted.
    // These are custom macro names used by a handful of corpus fixtures, not
    // standard LaTeX the engine should understand. The list must only shrink.
    const KNOWN_UNSUPPORTED: &[&str] = &[
        "endExp", "err", "fracerr", "sqrterr", "startExp", "suberr", "superr",
    ];

    let allow: BTreeSet<&str> = KNOWN_UNSUPPORTED.iter().copied().collect();
    let (found, parse_failures) = unsupported_commands_in("tests/data/katex_corpus.json");

    assert!(
        parse_failures.is_empty(),
        "lenient parsing of the KaTeX corpus produced hard errors: {parse_failures:?}"
    );

    let novel: Vec<&str> = found
        .iter()
        .map(String::as_str)
        .filter(|c| !allow.contains(c))
        .collect();
    assert!(
        novel.is_empty(),
        "new unsupported LaTeX commands are being rendered as literal text \
         (add support for them, or extend KNOWN_UNSUPPORTED only if intentional): {novel:?}"
    );

    // Keep the baseline honest: a command that is now supported must be dropped
    // from the allowlist so it cannot silently regress later.
    let stale: Vec<&str> = allow
        .iter()
        .copied()
        .filter(|c| !found.contains(*c))
        .collect();
    assert!(
        stale.is_empty(),
        "KNOWN_UNSUPPORTED lists commands that are now handled; remove them: {stale:?}"
    );
}

/// The full corpus must render to SVG without error (no panics, no `Err`).
#[cfg(feature = "render")]
#[test]
fn katex_corpus_renders_without_error() {
    let data = std::fs::read_to_string("tests/data/katex_corpus.json")
        .expect("read tests/data/katex_corpus.json");
    let formulas = parse_formulas(&data);

    let mut failures: Vec<(String, String)> = Vec::new();
    for formula in &formulas {
        let config = Config::default()
            .with_strictness(Strictness::Lenient)
            .with_display(if formula.display {
                DisplayStyle::Block
            } else {
                DisplayStyle::Inline
            });
        match from_latex_with(&formula.tex, &config) {
            Err(e) => failures.push((formula.name.clone(), format!("parse: {e}"))),
            Ok(result) => {
                if let Err(e) = result.to_svg() {
                    failures.push((formula.name.clone(), format!("render: {e}")));
                }
            },
        }
    }
    assert!(
        failures.is_empty(),
        "corpus formulas failed to render: {failures:?}"
    );
}

/// Honest output-level gate: the AST-heuristic `find_unsupported` test cannot
/// see internal sentinels or silent drops, so this asserts directly on rendered
/// MathML for the failure modes the assessment called out.
#[test]
fn output_never_leaks_infix_sentinel() {
    let config = Config::default()
        .with_strictness(Strictness::Lenient)
        .with_display(DisplayStyle::Inline);
    // Unbraced, braced, and *chained* infix operators must all build fractions
    // and never leak a `__infix_*__` sentinel into output.
    for tex in [
        "a \\over b",
        "a \\atop b",
        "x \\above 1pt y",
        "n \\choose k",
        "{a \\over b}",
        "1 \\over 2 \\over 3",
        "a + b \\over c + d",
        "\\frac{a \\over b}{c}",
    ] {
        let mathml = from_latex_with(tex, &config).unwrap().to_mathml().unwrap();
        assert!(
            !mathml.contains("__infix"),
            "infix sentinel leaked for {tex:?}: {mathml}"
        );
        assert!(
            mathml.contains("<mfrac"),
            "expected a fraction for {tex:?}: {mathml}"
        );
    }
}

/// Argument-bearing wrapper commands must preserve their *visible* argument
/// (they may drop the URL / id / class metadata, but not the rendered content).
#[test]
fn wrapper_commands_preserve_visible_content() {
    let config = Config::default()
        .with_strictness(Strictness::Lenient)
        .with_display(DisplayStyle::Inline);
    // Use a single-letter visible argument so it survives as one <mi>Z</mi>.
    for tex in [
        "\\href{http://example.com}{Z}",
        "\\htmlId{theid}{Z}",
        "\\htmlClass{cls}{Z}",
        "\\texttt{Z}",
    ] {
        let mathml = from_latex_with(tex, &config).unwrap().to_mathml().unwrap();
        assert!(
            mathml.contains('Z'),
            "visible content dropped for {tex:?}: {mathml}"
        );
    }
}

/// FM3: metadata-dropping macros must emit a reachable parse Warning.
#[test]
fn metadata_dropping_macros_warn() {
    for (tex, needle) in [
        ("\\href{http://x}{y}", "href"),
        ("\\includegraphics{a.png}", "includegraphics"),
        ("\\htmlId{q}{z}", "htmlId"),
    ] {
        let result = from_latex_with(tex, &Config::default()).unwrap();
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.message.contains(needle)),
            "expected a drop warning for {tex:?}, got {:?}",
            result.diagnostics()
        );
    }
    // Plain input produces no diagnostics.
    assert!(
        from_latex_with("x + y", &Config::default())
            .unwrap()
            .diagnostics()
            .is_empty()
    );
}

/// FM4: `alignat` must align like `alignedat` (columnalign present), not fall
/// through to the array fallback (which drops the column-count and alignment).
#[test]
fn alignat_aligns_like_alignedat() {
    let mathml = from_latex_with(
        "\\begin{alignat}{2} a&=b & c&=d \\\\ e&=f & g&=h \\end{alignat}",
        &Config::default(),
    )
    .unwrap()
    .to_mathml()
    .unwrap();
    assert!(
        mathml.contains("columnalign"),
        "alignat lost column alignment: {mathml}"
    );
}
