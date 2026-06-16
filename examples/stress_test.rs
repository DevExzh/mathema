//! Comprehensive stress test for the mathema rendering pipeline.
//!
//! Tests a wide variety of LaTeX formulas through both SVG and PNG paths,
//! checking for panics, valid output structure, and positive dimensions.
//!
//! Run with: cargo run --features raster --example stress_test

use mathema::{Format, convert, from_latex};
use std::panic;

/// A single test case: category name, LaTeX source, short label.
struct TestCase {
    category: &'static str,
    label: &'static str,
    latex: &'static str,
}

fn test_cases() -> Vec<TestCase> {
    let mut cases = Vec::new();

    // ── Simple expressions ──────────────────────────────────────────────
    for (label, latex) in [
        ("number", "42"),
        ("identifier", "x"),
        ("addition", "a + b"),
        ("subtraction", "a - b"),
        ("multiplication", "a \\cdot b"),
        ("division", "a / b"),
        ("equality", "x = y"),
        ("mixed_arith", "2x + 3y - z = 0"),
        ("negative", "-x"),
        ("decimal", "3.14"),
    ] {
        cases.push(TestCase {
            category: "simple",
            label,
            latex,
        });
    }

    // ── Greek letters and symbols ───────────────────────────────────────
    for (label, latex) in [
        ("lower_greek", r"\alpha \beta \gamma \delta \epsilon"),
        ("more_lower", r"\zeta \eta \theta \iota \kappa"),
        ("lambda_to_rho", r"\lambda \mu \nu \xi \pi \rho"),
        (
            "sigma_to_omega",
            r"\sigma \tau \upsilon \phi \chi \psi \omega",
        ),
        (
            "upper_greek",
            r"\Gamma \Delta \Theta \Lambda \Xi \Pi \Sigma \Phi \Psi \Omega",
        ),
        (
            "var_greek",
            r"\varepsilon \vartheta \varpi \varrho \varsigma \varphi",
        ),
        ("infinity", r"\infty"),
        ("misc_symbols", r"\partial \nabla \forall \exists \neg \ell"),
        (
            "arrows",
            r"\leftarrow \rightarrow \Leftrightarrow \uparrow \downarrow",
        ),
        (
            "relations",
            r"\leq \geq \neq \approx \sim \equiv \propto \subset \supset",
        ),
    ] {
        cases.push(TestCase {
            category: "greek_symbols",
            label,
            latex,
        });
    }

    // ── Fractions ───────────────────────────────────────────────────────
    for (label, latex) in [
        ("simple_frac", r"\frac{a}{b}"),
        ("frac_with_expr", r"\frac{x+1}{x-1}"),
        ("nested_frac", r"\frac{1}{\frac{2}{3}}"),
        ("double_nested", r"\frac{\frac{a}{b}}{\frac{c}{d}}"),
        (
            "continued_frac",
            r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+x}}}",
        ),
        ("dfrac", r"\dfrac{a}{b}"),
        ("tfrac", r"\tfrac{a}{b}"),
        ("binom", r"\binom{n}{k}"),
        ("frac_in_sum", r"\sum_{k=0}^n \frac{1}{k!}"),
        (
            "deeply_nested_frac",
            r"\frac{1}{2+\frac{3}{4+\frac{5}{6+\frac{7}{8}}}}",
        ),
    ] {
        cases.push(TestCase {
            category: "fractions",
            label,
            latex,
        });
    }

    // ── Scripts ─────────────────────────────────────────────────────────
    for (label, latex) in [
        ("superscript", "x^2"),
        ("subscript", "x_i"),
        ("both_scripts", "x_i^2"),
        ("nested_super", "x^{y^{z}}"),
        ("nested_sub", "a_{b_{c}}"),
        ("deep_super", "x^{y^{z^{w^{v}}}}"),
        ("prime", "f'(x)"),
        ("double_prime", "f''(x)"),
        ("sub_with_expr", "T_{i+1}"),
        ("complex_scripts", "a_1^2 + b_2^3 + c_3^4"),
    ] {
        cases.push(TestCase {
            category: "scripts",
            label,
            latex,
        });
    }

    // ── Radicals ────────────────────────────────────────────────────────
    for (label, latex) in [
        ("sqrt", r"\sqrt{x}"),
        ("sqrt_expr", r"\sqrt{x^2 + y^2}"),
        ("cube_root", r"\sqrt[3]{x}"),
        ("nth_root", r"\sqrt[n]{a}"),
        ("nested_sqrt", r"\sqrt{\sqrt{x}}"),
        ("deep_nested_sqrt", r"\sqrt{\sqrt{\sqrt{x}}}"),
        ("sqrt_frac", r"\sqrt{\frac{a}{b}}"),
        ("frac_sqrt", r"\frac{\sqrt{a}}{\sqrt{b}}"),
        ("sqrt_sum", r"\sqrt{\sum_{i=1}^n x_i^2}"),
        ("quadratic", r"\frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
    ] {
        cases.push(TestCase {
            category: "radicals",
            label,
            latex,
        });
    }

    // ── Large operators with limits ─────────────────────────────────────
    for (label, latex) in [
        ("sum_limits", r"\sum_{i=1}^{n} x_i"),
        ("prod_limits", r"\prod_{k=1}^{n} k"),
        ("integral", r"\int_0^1 f(x)\,dx"),
        ("double_integral", r"\iint_D f(x,y)\,dA"),
        ("triple_integral", r"\iiint_V f\,dV"),
        ("oint", r"\oint_C \vec{F}\cdot d\vec{r}"),
        ("coprod", r"\coprod_{i \in I} X_i"),
        ("bigcup", r"\bigcup_{i=1}^n A_i"),
        ("bigcap", r"\bigcap_{i=1}^n A_i"),
        ("lim", r"\lim_{x \to \infty} f(x)"),
        ("limsup", r"\limsup_{n\to\infty} a_n"),
        ("sum_no_limits", r"\sum x_i"),
    ] {
        cases.push(TestCase {
            category: "large_operators",
            label,
            latex,
        });
    }

    // ── Accents ─────────────────────────────────────────────────────────
    for (label, latex) in [
        ("hat", r"\hat{x}"),
        ("tilde", r"\tilde{x}"),
        ("vec", r"\vec{v}"),
        ("bar", r"\bar{x}"),
        ("dot", r"\dot{x}"),
        ("ddot", r"\ddot{x}"),
        ("overline", r"\overline{AB}"),
        ("underline", r"\underline{x}"),
        ("overbrace", r"\overbrace{a+b+c}^{n}"),
        ("underbrace", r"\underbrace{x+y+z}_{3}"),
        ("widehat", r"\widehat{ABC}"),
        ("widetilde", r"\widetilde{XYZ}"),
        ("overrightarrow", r"\overrightarrow{AB}"),
        ("overleftarrow", r"\overleftarrow{AB}"),
    ] {
        cases.push(TestCase {
            category: "accents",
            label,
            latex,
        });
    }

    // ── Delimiters ──────────────────────────────────────────────────────
    for (label, latex) in [
        ("parens", r"\left( x \right)"),
        ("brackets", r"\left[ x \right]"),
        ("braces", r"\left\{ x \right\}"),
        ("pipes", r"\left| x \right|"),
        ("double_pipes", r"\left\| x \right\|"),
        ("angle", r"\left\langle x \right\rangle"),
        ("floor", r"\left\lfloor x \right\rfloor"),
        ("ceil", r"\left\lceil x \right\rceil"),
        ("mixed", r"\left( \frac{a}{b} \right]"),
        (
            "nested_delim",
            r"\left(\left[\left\{x\right\}\right]\right)",
        ),
        (
            "tall_delim",
            r"\left(\frac{\frac{a}{b}}{\frac{c}{d}}\right)",
        ),
        ("dot_delim", r"\left. \frac{dy}{dx} \right|_{x=0}"),
    ] {
        cases.push(TestCase {
            category: "delimiters",
            label,
            latex,
        });
    }

    // ── Matrices and arrays ─────────────────────────────────────────────
    for (label, latex) in [
        ("pmatrix", r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}"),
        ("bmatrix", r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}"),
        ("vmatrix", r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}"),
        ("Bmatrix", r"\begin{Bmatrix} x \\ y \end{Bmatrix}"),
        (
            "cases",
            r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & x = 0 \\ -1 & x < 0 \end{cases}",
        ),
        (
            "aligned",
            r"\begin{aligned} x &= a + b \\ y &= c + d \end{aligned}",
        ),
        (
            "matrix_3x3",
            r"\begin{pmatrix} 1 & 2 & 3 \\ 4 & 5 & 6 \\ 7 & 8 & 9 \end{pmatrix}",
        ),
        (
            "small_matrix",
            r"\left(\begin{smallmatrix} a & b \\ c & d \end{smallmatrix}\right)",
        ),
        (
            "cases_nested",
            r"\begin{cases} \frac{1}{x} & x \neq 0 \\ 0 & x = 0 \end{cases}",
        ),
    ] {
        cases.push(TestCase {
            category: "matrices",
            label,
            latex,
        });
    }

    // ── Colors ──────────────────────────────────────────────────────────
    for (label, latex) in [
        ("textcolor_red", r"\textcolor{red}{x}"),
        ("textcolor_blue", r"\textcolor{blue}{y^2}"),
        ("color_green", r"{\color{green} a + b}"),
        (
            "mixed_colors",
            r"\textcolor{red}{x} + \textcolor{blue}{y} = \textcolor{purple}{z}",
        ),
        ("color_frac", r"\textcolor{red}{\frac{a}{b}}"),
        ("colorbox", r"\colorbox{yellow}{x}"),
        ("fcolorbox", r"\fcolorbox{blue}{pink}{z}"),
    ] {
        cases.push(TestCase {
            category: "colors",
            label,
            latex,
        });
    }

    // ── Decorators ──────────────────────────────────────────────────────
    for (label, latex) in [
        ("boxed", r"\boxed{E = mc^2}"),
        ("cancel", r"\cancel{x}"),
        ("bcancel", r"\bcancel{y}"),
        ("xcancel", r"\xcancel{z}"),
        ("phantom", r"a + \phantom{b} + c"),
        ("hphantom", r"a + \hphantom{b} + c"),
        ("vphantom", r"a + \vphantom{b} + c"),
        ("boxed_frac", r"\boxed{\frac{1}{2}}"),
    ] {
        cases.push(TestCase {
            category: "decorators",
            label,
            latex,
        });
    }

    // ── Font styles ─────────────────────────────────────────────────────
    for (label, latex) in [
        ("mathbb", r"\mathbb{R}"),
        ("mathcal", r"\mathcal{L}"),
        ("mathfrak", r"\mathfrak{g}"),
        ("mathrm", r"\mathrm{d}x"),
        ("mathbf", r"\mathbf{v}"),
        ("mathit", r"\mathit{x}"),
        ("mathsf", r"\mathsf{T}"),
        ("mathtt", r"\mathtt{code}"),
        ("text", r"x \text{ for all } x"),
        ("boldsymbol", r"\boldsymbol{\alpha}"),
    ] {
        cases.push(TestCase {
            category: "font_styles",
            label,
            latex,
        });
    }

    // ── Spacing commands ────────────────────────────────────────────────
    for (label, latex) in [
        ("quad", r"a \quad b"),
        ("qquad", r"a \qquad b"),
        ("thickspace", r"a \; b"),
        ("medspace", r"a \: b"),
        ("thinspace", r"a \, b"),
        ("negspace", r"a \! b"),
        ("explicit_hspace", r"a \hspace{1em} b"),
    ] {
        cases.push(TestCase {
            category: "spacing",
            label,
            latex,
        });
    }

    // ── Display mode ────────────────────────────────────────────────────
    for (label, latex) in [
        ("displaystyle_sum", r"\displaystyle \sum_{i=1}^n x_i"),
        ("displaystyle_frac", r"\displaystyle \frac{a}{b}"),
        ("textstyle_sum", r"\textstyle \sum_{i=1}^n"),
        ("scriptstyle", r"\scriptstyle x^2"),
        ("scriptscriptstyle", r"\scriptscriptstyle x"),
    ] {
        cases.push(TestCase {
            category: "display_mode",
            label,
            latex,
        });
    }

    // ── Edge cases ──────────────────────────────────────────────────────
    for (label, latex) in [
        ("empty", ""),
        ("single_char", "x"),
        ("single_number", "1"),
        ("empty_group", "{}"),
        ("nested_empty_groups", "{{{}}}"),
        ("only_space", " "),
        ("many_operators", "a + b - c \\cdot d / e"),
        ("long_subscript", "x_{abcdefg}"),
        ("deep_nesting", "{{{{{{x}}}}}}"),
        ("adjacent_scripts", "x^2_3 + y^4_5"),
        (
            "large_expression",
            r"\sum_{i=0}^{\infty}\frac{(-1)^i}{(2i+1)!}x^{2i+1}",
        ),
    ] {
        cases.push(TestCase {
            category: "edge_cases",
            label,
            latex,
        });
    }

    // ── Real-world formulas ─────────────────────────────────────────────
    for (label, latex) in [
        ("euler_identity", r"e^{i\pi} + 1 = 0"),
        ("pythagorean", r"a^2 + b^2 = c^2"),
        (
            "gauss_law",
            r"\nabla \cdot \vec{E} = \frac{\rho}{\epsilon_0}",
        ),
        (
            "schrodinger",
            r"i\hbar\frac{\partial}{\partial t}\Psi = \hat{H}\Psi",
        ),
        (
            "maxwell",
            r"\nabla \times \vec{B} = \mu_0\vec{J} + \mu_0\epsilon_0\frac{\partial \vec{E}}{\partial t}",
        ),
        (
            "taylor",
            r"f(x) = \sum_{n=0}^{\infty} \frac{f^{(n)}(a)}{n!}(x-a)^n",
        ),
        (
            "fourier",
            r"f(x) = \frac{a_0}{2} + \sum_{n=1}^{\infty}\left(a_n\cos\frac{n\pi x}{L} + b_n\sin\frac{n\pi x}{L}\right)",
        ),
        (
            "einstein_field",
            r"R_{\mu\nu} - \frac{1}{2}Rg_{\mu\nu} + \Lambda g_{\mu\nu} = \frac{8\pi G}{c^4}T_{\mu\nu}",
        ),
        (
            "gaussian_integral",
            r"\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}",
        ),
        (
            "stirling",
            r"n! \approx \sqrt{2\pi n}\left(\frac{n}{e}\right)^n",
        ),
    ] {
        cases.push(TestCase {
            category: "real_world",
            label,
            latex,
        });
    }

    // ── Extensible arrows ───────────────────────────────────────────────
    for (label, latex) in [
        ("xrightarrow", r"A \xrightarrow{f} B"),
        ("xleftarrow", r"A \xleftarrow{g} B"),
        ("xrightarrow_sub", r"A \xrightarrow[below]{above} B"),
        ("xleftarrow_sub", r"A \xleftarrow[below]{above} B"),
        ("overset", r"\overset{n}{X}"),
        ("underset", r"\underset{k}{Y}"),
    ] {
        cases.push(TestCase {
            category: "extensible_arrows",
            label,
            latex,
        });
    }

    // ── Operators and functions ──────────────────────────────────────────
    for (label, latex) in [
        ("sin_cos", r"\sin x + \cos y"),
        ("tan_log", r"\tan\theta + \log_2 n"),
        ("exp_ln", r"\exp(x) + \ln y"),
        ("min_max", r"\min(a,b) + \max(c,d)"),
        ("det", r"\det(A)"),
        ("gcd_lcm", r"\gcd(a,b)"),
        ("mod", r"a \bmod b"),
        ("lim_notation", r"\lim_{x \to 0} \frac{\sin x}{x} = 1"),
    ] {
        cases.push(TestCase {
            category: "operators",
            label,
            latex,
        });
    }

    // ── Dots ────────────────────────────────────────────────────────────
    for (label, latex) in [
        ("cdots", r"a_1, a_2, \cdots, a_n"),
        ("ldots", r"a_1, a_2, \ldots, a_n"),
        ("vdots", r"\vdots"),
        ("ddots", r"\ddots"),
        ("dots_mixed", r"1 + 2 + \cdots + n"),
    ] {
        cases.push(TestCase {
            category: "dots",
            label,
            latex,
        });
    }

    cases
}

/// Result of testing a single formula.
struct TestResult {
    category: &'static str,
    label: &'static str,
    latex: &'static str,
    svg_ok: bool,
    svg_detail: String,
    png_ok: bool,
    png_detail: String,
}

fn validate_svg(svg: &str) -> std::result::Result<(), String> {
    if !svg.starts_with("<svg") {
        return Err(format!(
            "SVG does not start with '<svg' (starts with {:?})",
            &svg[..svg.len().min(30)]
        ));
    }
    if !svg.trim_end().ends_with("</svg>") {
        return Err("SVG does not end with '</svg>'".into());
    }

    // Check for width/height attributes
    let width = extract_attr(svg, "width");
    let height = extract_attr(svg, "height");

    match (width, height) {
        (Some(w), Some(h)) => {
            let w_val = parse_dimension(&w);
            let h_val = parse_dimension(&h);
            if w_val <= 0.0 {
                return Err(format!("SVG width is not positive: {}", w));
            }
            if h_val <= 0.0 {
                return Err(format!("SVG height is not positive: {}", h));
            }
            Ok(())
        },
        (None, _) => Err("SVG missing 'width' attribute".into()),
        (_, None) => Err("SVG missing 'height' attribute".into()),
    }
}

fn extract_attr(svg: &str, attr_name: &str) -> Option<String> {
    // Look in the opening <svg ...> tag
    let tag_end = svg.find('>')?;
    let tag = &svg[..tag_end];
    let pattern = format!("{}=\"", attr_name);
    let start = tag.find(&pattern)? + pattern.len();
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

fn parse_dimension(s: &str) -> f64 {
    // Strip trailing units like "px", "em", etc.
    let numeric: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    numeric.parse::<f64>().unwrap_or(0.0)
}

fn run_test(tc: &TestCase) -> TestResult {
    // Test SVG rendering (via convert)
    let (svg_ok, svg_detail) =
        match panic::catch_unwind(|| convert(tc.latex, Format::LaTeX, Format::Svg)) {
            Ok(Ok(svg)) => match validate_svg(&svg) {
                Ok(()) => (true, format!("OK ({} bytes)", svg.len())),
                Err(e) => (false, format!("Invalid SVG: {}", e)),
            },
            Ok(Err(e)) => (false, format!("Error: {}", e)),
            Err(_) => (false, "PANIC during SVG rendering".into()),
        };

    // Test PNG rendering (via from_latex builder)
    let (png_ok, png_detail) =
        match panic::catch_unwind(|| from_latex(tc.latex).and_then(|r| r.to_png())) {
            Ok(Ok(png)) => {
                if png.is_empty() {
                    (false, "PNG is empty (0 bytes)".into())
                } else {
                    // Verify PNG magic bytes
                    if png.len() >= 8 && &png[..4] == b"\x89PNG" {
                        (true, format!("OK ({} bytes)", png.len()))
                    } else {
                        (
                            false,
                            format!(
                                "Not valid PNG data ({} bytes, header: {:?})",
                                png.len(),
                                &png[..png.len().min(8)]
                            ),
                        )
                    }
                }
            },
            Ok(Err(e)) => (false, format!("Error: {}", e)),
            Err(_) => (false, "PANIC during PNG rendering".into()),
        };

    TestResult {
        category: tc.category,
        label: tc.label,
        latex: tc.latex,
        svg_ok,
        svg_detail,
        png_ok,
        png_detail,
    }
}

fn main() {
    let cases = test_cases();
    let total = cases.len();
    println!("Running stress test with {} formulas...\n", total);

    let results: Vec<TestResult> = cases.iter().map(run_test).collect();

    // Print results grouped by category
    let mut current_category = "";
    let mut svg_pass = 0usize;
    let mut svg_fail = 0usize;
    let mut png_pass = 0usize;
    let mut png_fail = 0usize;
    let mut failures: Vec<&TestResult> = Vec::new();

    for r in &results {
        if r.category != current_category {
            current_category = r.category;
            println!("--- {} ---", current_category);
        }

        let svg_icon = if r.svg_ok { "PASS" } else { "FAIL" };
        let png_icon = if r.png_ok { "PASS" } else { "FAIL" };

        if r.svg_ok {
            svg_pass += 1;
        } else {
            svg_fail += 1;
        }
        if r.png_ok {
            png_pass += 1;
        } else {
            png_fail += 1;
        }

        println!(
            "  [{}/{}] {:<30} SVG: {:<40} PNG: {}",
            svg_icon, png_icon, r.label, r.svg_detail, r.png_detail
        );

        if !r.svg_ok || !r.png_ok {
            failures.push(r);
        }
    }

    // Summary
    println!("\n{}", "=".repeat(80));
    println!("STRESS TEST SUMMARY");
    println!("{}", "=".repeat(80));
    println!("Total formulas tested: {}", total);
    println!("SVG: {} passed, {} failed", svg_pass, svg_fail);
    println!("PNG: {} passed, {} failed", png_pass, png_fail);
    println!("{}", "-".repeat(80));

    if failures.is_empty() {
        println!("ALL TESTS PASSED!");
    } else {
        println!("\nFAILURES ({}):", failures.len());
        println!("{}", "-".repeat(80));
        for f in &failures {
            println!("  [{}::{}] LaTeX: {}", f.category, f.label, f.latex);
            if !f.svg_ok {
                println!("    SVG: {}", f.svg_detail);
            }
            if !f.png_ok {
                println!("    PNG: {}", f.png_detail);
            }
        }
    }

    // Exit with non-zero status if there are failures
    if !failures.is_empty() {
        std::process::exit(1);
    }
}
