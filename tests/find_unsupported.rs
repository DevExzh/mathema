#![cfg(feature = "render")]

use std::collections::{BTreeMap, BTreeSet};

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
        let config = Config {
            strictness: Strictness::Lenient,
            display: if formula.display {
                DisplayStyle::Block
            } else {
                DisplayStyle::Inline
            },
            ..Config::default()
        };

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

    let config = Config {
        strictness: Strictness::Lenient,
        display: DisplayStyle::Inline,
        ..Config::default()
    };

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
