/// Comprehensive visual quality audit — generates hi-res PNG for every major formula type.
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;

fn main() {
    let config = Config::default();
    // Group formulas by category for systematic testing
    let cases: Vec<(&str, &str)> = vec![
        // === Basic operations ===
        ("basic_add", r"a + b"),
        ("basic_mul", r"a \times b"),
        ("basic_div", r"a \div b"),
        ("basic_neg", r"-a + b"),
        ("basic_eq", r"a = b"),
        ("basic_ineq", r"a < b \leq c"),
        // === Fractions ===
        ("frac_simple", r"\frac{a}{b}"),
        ("frac_nested", r"\frac{\frac{a}{b}}{\frac{c}{d}}"),
        ("frac_display", r"\displaystyle\frac{1}{2}"),
        ("frac_continued", r"\frac{1}{1+\frac{1}{2+\frac{1}{3}}}"),
        ("frac_dfrac", r"\dfrac{x^2+1}{x-1}"),
        // === Superscripts and subscripts ===
        ("sup_simple", r"x^2"),
        ("sub_simple", r"x_i"),
        ("subsup", r"x_i^2"),
        ("sup_chain", r"a^{b^{c^d}}"),
        ("sub_nested", r"x_{i_{j_k}}"),
        // === Radicals ===
        ("sqrt_simple", r"\sqrt{x}"),
        ("sqrt_complex", r"\sqrt{x^2 + y^2}"),
        ("sqrt_nested", r"\sqrt{\sqrt{x}}"),
        ("cbrt", r"\sqrt[3]{x}"),
        ("nthroot", r"\sqrt[n]{a^n}"),
        ("sqrt_frac", r"\sqrt{\frac{a}{b}}"),
        // === Summation and products ===
        ("sum_display", r"\displaystyle\sum_{i=0}^{n} x_i"),
        ("prod_display", r"\displaystyle\prod_{k=1}^{n} k"),
        ("sum_inline", r"\sum_{i=0}^n x_i"),
        ("sum_double", r"\sum_{i=1}^m \sum_{j=1}^n a_{ij}"),
        // === Integrals ===
        ("int_display", r"\displaystyle\int_0^\infty e^{-x} dx"),
        ("int_double", r"\iint_D f(x,y)\,dA"),
        (
            "int_limits",
            r"\displaystyle\int_{-\infty}^{\infty} e^{-x^2} dx",
        ),
        ("oint", r"\oint_C \vec{F} \cdot d\vec{r}"),
        // === Matrices ===
        (
            "matrix_paren",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
        ),
        (
            "matrix_bracket",
            r"\begin{bmatrix} 1 & 0 \\ 0 & 1 \end{bmatrix}",
        ),
        (
            "matrix_3x3",
            r"\begin{pmatrix} 1 & 2 & 3 \\ 4 & 5 & 6 \\ 7 & 8 & 9 \end{pmatrix}",
        ),
        (
            "matrix_vmatrix",
            r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}",
        ),
        // === Delimiters ===
        ("delim_paren", r"\left(\frac{a}{b}\right)"),
        ("delim_bracket", r"\left[\frac{a}{b}\right]"),
        ("delim_brace", r"\left\{\frac{a}{b}\right\}"),
        ("delim_floor", r"\left\lfloor x \right\rfloor"),
        ("delim_ceil", r"\left\lceil x \right\rceil"),
        ("delim_angle", r"\langle x, y \rangle"),
        ("delim_pipe", r"\left| x \right|"),
        ("delim_dpipe", r"\left\| x \right\|"),
        (
            "delim_nested",
            r"\left(\left[\left\{\frac{a}{b}\right\}\right]\right)",
        ),
        // === Greek letters ===
        ("greek_lower", r"\alpha \beta \gamma \delta \epsilon"),
        ("greek_upper", r"\Gamma \Delta \Theta \Lambda \Sigma"),
        ("greek_fancy", r"\varphi \varepsilon \vartheta"),
        // === Accents ===
        ("accent_hat", r"\hat{x}"),
        ("accent_widehat", r"\widehat{xyz}"),
        ("accent_bar", r"\bar{x}"),
        ("accent_vec", r"\vec{v}"),
        ("accent_dot", r"\dot{x}"),
        ("accent_ddot", r"\ddot{x}"),
        ("accent_tilde", r"\tilde{x}"),
        ("accent_widetilde", r"\widetilde{xyz}"),
        // === Environments ===
        (
            "cases",
            r"\begin{cases} x & \text{if } x > 0 \\ -x & \text{otherwise} \end{cases}",
        ),
        (
            "aligned",
            r"\begin{aligned} x &= a + b \\ y &= c + d \end{aligned}",
        ),
        // === Text and operators ===
        ("text_in_math", r"\text{if } x > 0 \text{ then } y"),
        ("operatorname", r"\operatorname{lim}_{x \to 0} f(x)"),
        ("named_op", r"\sin x + \cos x + \tan x"),
        ("log", r"\log_2 n"),
        // === Spacing ===
        ("spacing_thin", r"a\,b"),
        ("spacing_medium", r"a\;b"),
        ("spacing_quad", r"a\quad b"),
        ("spacing_qquad", r"a\qquad b"),
        ("spacing_neg", r"a\!b"),
        // === Special constructs ===
        ("boxed", r"\boxed{E = mc^2}"),
        ("cancel", r"\cancel{x} + y"),
        ("phantom", r"a\phantom{xyz}b"),
        ("color", r"\color{red}{x} + \color{blue}{y}"),
        ("underbrace", r"\underbrace{a + b + c}_{\text{sum}}"),
        ("overbrace", r"\overbrace{x + y}^{z}"),
        ("overline", r"\overline{x + y}"),
        ("underline", r"\underline{x + y}"),
        // === Famous formulas ===
        ("euler", r"e^{i\pi} + 1 = 0"),
        ("quadratic", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
        ("gauss", r"\int_{-\infty}^{\infty} e^{-x^2} dx = \sqrt{\pi}"),
        ("binomial", r"\binom{n}{k} = \frac{n!}{k!(n-k)!}"),
        ("taylor", r"e^x = \sum_{n=0}^{\infty} \frac{x^n}{n!}"),
        (
            "maxwell",
            r"\nabla \times \vec{E} = -\frac{\partial \vec{B}}{\partial t}",
        ),
        (
            "schrodinger",
            r"i\hbar\frac{\partial}{\partial t}\Psi = \hat{H}\Psi",
        ),
        ("pythagoras", r"a^2 + b^2 = c^2"),
    ];

    std::fs::create_dir_all("test_matrix/audit").ok();
    let mut ok_count = 0;
    let mut fail_count = 0;
    let mut fail_list = Vec::new();

    for (name, latex) in &cases {
        match LatexReader::parse_compact(latex, &config) {
            Ok(result) => {
                use mathema::render::RenderOptions;
                use mathema::render::raster::{
                    ImageFormat, RasterOptions, render_to_image_compact,
                };
                let opts = RasterOptions {
                    render: RenderOptions {
                        font_size_px: 48.0,
                        display_mode: Some(true),
                        fg_color: "#000000".into(),
                        font_family: "KaTeX_Main, Latin Modern Math, STIX Two Math, serif".into(),
                    },
                    format: ImageFormat::Png,
                    scale: 2.0,
                    background: Some([255, 255, 255, 255]),
                    quality: 90,
                };
                match render_to_image_compact(&result.expr, &opts) {
                    Ok(png) => {
                        std::fs::write(format!("test_matrix/audit/{name}.png"), &png).unwrap();
                        ok_count += 1;
                    },
                    Err(e) => {
                        eprintln!("RENDER FAIL: {name}: {e}");
                        fail_count += 1;
                        fail_list.push((*name, format!("render: {e}")));
                    },
                }
            },
            Err(e) => {
                eprintln!("PARSE FAIL: {name}: {e}");
                fail_count += 1;
                fail_list.push((*name, format!("parse: {e}")));
            },
        }
    }

    println!("\n=== Visual Audit Results ===");
    println!("OK: {ok_count}/{} formulas rendered", ok_count + fail_count);
    if !fail_list.is_empty() {
        println!("FAILED ({fail_count}):");
        for (name, err) in &fail_list {
            println!("  - {name}: {err}");
        }
    }
    println!("Output in test_matrix/audit/");
}
