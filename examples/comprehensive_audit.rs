use mathema::convert;
/// Comprehensive rendering quality audit.
/// Generates PNG images at 2x scale for all formula categories.
use mathema::{ImageFormat, RasterOptions, RenderOptions};

fn main() {
    let formulas: Vec<(&str, &str)> = vec![
        // Basic operators
        ("basic_add", r"a + b"),
        ("basic_mul", r"a \times b"),
        ("basic_div", r"a \div b"),
        ("basic_pm", r"a \pm b"),
        ("basic_cdot", r"a \cdot b"),
        // Fractions
        ("frac_simple", r"\frac{a}{b}"),
        ("frac_nested", r"\frac{\frac{a}{b}}{\frac{c}{d}}"),
        ("frac_display", r"\dfrac{x^2 + 1}{x - 1}"),
        ("frac_text", r"x + \tfrac{1}{2}"),
        (
            "frac_continued",
            r"a_0 + \cfrac{1}{a_1 + \cfrac{1}{a_2 + \cfrac{1}{a_3}}}",
        ),
        // Superscripts and subscripts
        ("script_sup", r"x^2"),
        ("script_sub", r"x_i"),
        ("script_subsup", r"x_i^2"),
        ("script_nested", r"e^{e^{e^x}}"),
        ("script_deep", r"x_{a_{b_{c}}}"),
        ("script_prime", r"f'(x)"),
        ("script_double_prime", r"f''(x)"),
        // Radicals
        ("sqrt_simple", r"\sqrt{x}"),
        ("sqrt_complex", r"\sqrt{x^2 + y^2}"),
        ("sqrt_nested", r"\sqrt{\sqrt{\sqrt{x}}}"),
        ("sqrt_nth", r"\sqrt[3]{x}"),
        ("sqrt_frac", r"\sqrt{\frac{a}{b}}"),
        // Large operators
        ("sum_simple", r"\sum_{i=0}^{n} i"),
        ("sum_display", r"\sum_{k=1}^{\infty} \frac{1}{k^2}"),
        ("prod_display", r"\prod_{i=1}^{n} x_i"),
        ("int_simple", r"\int_0^1 f(x)\,dx"),
        ("int_double", r"\iint_D f(x,y)\,dA"),
        ("int_triple", r"\iiint_V f\,dV"),
        ("oint", r"\oint_C \vec{F} \cdot d\vec{r}"),
        ("coprod", r"\coprod_{i \in I} G_i"),
        ("bigcap", r"\bigcap_{i=1}^{n} A_i"),
        ("bigcup", r"\bigcup_{i=1}^{n} A_i"),
        // Delimiters
        ("delim_paren", r"\left( \frac{a}{b} \right)"),
        ("delim_bracket", r"\left[ \frac{a}{b} \right]"),
        ("delim_brace", r"\left\{ \frac{a}{b} \right\}"),
        ("delim_pipe", r"\left| \frac{a}{b} \right|"),
        ("delim_dpipe", r"\left\| \frac{a}{b} \right\|"),
        ("delim_angle", r"\left\langle \frac{a}{b} \right\rangle"),
        ("delim_floor", r"\left\lfloor x \right\rfloor"),
        ("delim_ceil", r"\left\lceil x \right\rceil"),
        (
            "delim_mixed",
            r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)",
        ),
        (
            "delim_tall",
            r"\left( \sum_{k=0}^{n} \binom{n}{k} x^k \right)",
        ),
        // Matrices
        (
            "matrix_2x2",
            r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
        ),
        (
            "matrix_3x3",
            r"\begin{bmatrix} 1 & 0 & 0 \\ 0 & 1 & 0 \\ 0 & 0 & 1 \end{bmatrix}",
        ),
        (
            "matrix_det",
            r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}",
        ),
        (
            "matrix_norm",
            r"\begin{Vmatrix} a & b \\ c & d \end{Vmatrix}",
        ),
        (
            "matrix_cases",
            r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & x = 0 \\ -1 & x < 0 \end{cases}",
        ),
        // Accents
        ("accent_hat", r"\hat{a}"),
        ("accent_tilde", r"\tilde{a}"),
        ("accent_bar", r"\bar{a}"),
        ("accent_vec", r"\vec{a}"),
        ("accent_dot", r"\dot{a}"),
        ("accent_ddot", r"\ddot{a}"),
        ("accent_wide_hat", r"\widehat{abc}"),
        ("accent_wide_tilde", r"\widetilde{xyz}"),
        ("accent_overline", r"\overline{AB}"),
        // Over/under
        ("overbrace", r"\overbrace{a + b + c}^{n}"),
        ("underbrace", r"\underbrace{a + b + c}_{n}"),
        ("overset", r"\overset{?}{=}"),
        ("underset", r"\underset{\text{def}}{=}"),
        ("overrightarrow", r"\overrightarrow{AB}"),
        ("overleftarrow", r"\overleftarrow{AB}"),
        // Text and font styles
        ("text_rm", r"\mathrm{var}"),
        ("text_bf", r"\mathbf{A}"),
        ("text_it", r"\mathit{text}"),
        ("text_bb", r"\mathbb{R}"),
        ("text_cal", r"\mathcal{L}"),
        ("text_frak", r"\mathfrak{g}"),
        ("text_tt", r"\mathtt{code}"),
        // Greek letters
        (
            "greek_lower",
            r"\alpha \beta \gamma \delta \epsilon \zeta \eta \theta",
        ),
        (
            "greek_upper",
            r"\Gamma \Delta \Theta \Lambda \Xi \Pi \Sigma \Omega",
        ),
        ("greek_var", r"\varepsilon \vartheta \varphi"),
        // Relations and arrows
        ("rel_eq", r"a = b \neq c \leq d \geq e"),
        ("rel_approx", r"a \approx b \sim c \equiv d"),
        ("rel_subset", r"A \subset B \subseteq C \supset D"),
        ("rel_in", r"x \in A \notin B"),
        ("arrow_right", r"a \rightarrow b \Rightarrow c"),
        ("arrow_left", r"a \leftarrow b \Leftarrow c"),
        ("arrow_lr", r"a \leftrightarrow b \Leftrightarrow c"),
        ("arrow_mapsto", r"x \mapsto f(x)"),
        ("xrightarrow", r"A \xrightarrow{\text{f}} B"),
        // Binomial and choose
        ("binom", r"\binom{n}{k}"),
        ("binom_display", r"\dbinom{n+1}{k}"),
        // Aligned environments
        (
            "aligned",
            r"\begin{aligned} x &= a + b \\ y &= c + d \end{aligned}",
        ),
        (
            "align_multi",
            r"\begin{aligned} f(x) &= x^2 + 2x + 1 \\ &= (x+1)^2 \end{aligned}",
        ),
        // Spacing
        ("spacing_thin", r"a\,b"),
        ("spacing_med", r"a\:b"),
        ("spacing_thick", r"a\;b"),
        ("spacing_quad", r"a\quad b"),
        ("spacing_neg", r"a\!b"),
        // Complex formulas
        ("euler", r"e^{i\pi} + 1 = 0"),
        ("quadratic", r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
        (
            "gauss",
            r"\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}",
        ),
        (
            "maxwell",
            r"\nabla \times \vec{E} = -\frac{\partial \vec{B}}{\partial t}",
        ),
        (
            "schrodinger",
            r"i\hbar\frac{\partial}{\partial t}\Psi = \hat{H}\Psi",
        ),
        (
            "taylor",
            r"f(x) = \sum_{n=0}^{\infty} \frac{f^{(n)}(a)}{n!}(x-a)^n",
        ),
        (
            "fourier",
            r"\hat{f}(\xi) = \int_{-\infty}^{\infty} f(x)\,e^{-2\pi i x \xi}\,dx",
        ),
        (
            "cauchy",
            r"f(a) = \frac{1}{2\pi i} \oint_\gamma \frac{f(z)}{z - a}\,dz",
        ),
        (
            "einstein",
            r"R_{\mu\nu} - \frac{1}{2}Rg_{\mu\nu} + \Lambda g_{\mu\nu} = \frac{8\pi G}{c^4}T_{\mu\nu}",
        ),
        (
            "stirling",
            r"n! \approx \sqrt{2\pi n}\left(\frac{n}{e}\right)^n",
        ),
        // Edge cases
        ("empty_frac", r"\frac{}{}"),
        ("single_char", r"x"),
        ("cancel", r"\cancel{x}"),
        ("color", r"\color{red}{x} + \color{blue}{y}"),
        ("boxed_expr", r"\boxed{E = mc^2}"),
        ("phantom", r"a\phantom{xyz}b"),
        // Additional complex
        ("limit", r"\lim_{x \to 0} \frac{\sin x}{x} = 1"),
        (
            "det_matrix",
            r"\det \begin{pmatrix} a & b \\ c & d \end{pmatrix} = ad - bc",
        ),
        ("log_exp", r"\log_2 x = \frac{\ln x}{\ln 2}"),
        ("trig", r"\sin^2\theta + \cos^2\theta = 1"),
        (
            "binomial_thm",
            r"(x+y)^n = \sum_{k=0}^{n} \binom{n}{k} x^{n-k} y^k",
        ),
        (
            "stokes",
            r"\int_{\partial \Omega} \omega = \int_{\Omega} d\omega",
        ),
    ];

    std::fs::create_dir_all("test_gallery/audit").expect("create dir");

    let mut html = String::from(
        r#"<!DOCTYPE html>
<html><head>
<meta charset="utf-8">
<title>Mathema Rendering Audit</title>
<style>
body { font-family: sans-serif; background: #f5f5f5; margin: 20px; }
.formula { display: inline-block; border: 1px solid #ccc; margin: 6px; padding: 10px;
           vertical-align: top; background: white; border-radius: 4px; }
.formula img { display: block; }
.formula .label { font-size: 11px; color: #666; margin-top: 4px; }
.formula .latex { font-size: 10px; color: #999; font-family: monospace;
                  max-width: 300px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.error { color: red; font-weight: bold; }
h1 { color: #333; }
</style>
</head><body>
<h1>Mathema Rendering Quality Audit</h1>
<p>Generated with scale=2x, font_size=20px, display mode</p>
"#,
    );

    let mut ok_count = 0;
    let mut err_count = 0;
    let mut errors = Vec::new();

    let options = RasterOptions {
        render: RenderOptions {
            font_size_px: 20.0,
            display_mode: Some(true),
            fg_color: "#000000".to_string(),
            ..RenderOptions::default()
        },
        format: ImageFormat::Png,
        scale: 2.0,
        background: Some([255, 255, 255, 255]),
        quality: 95,
    };

    for (name, latex) in &formulas {
        let result = convert::from_latex(latex);
        match result {
            Ok(res) => match res.to_png_with(&options) {
                Ok(bytes) => {
                    let path = format!("test_gallery/audit/{}.png", name);
                    std::fs::write(&path, &bytes).expect("write png");
                    html.push_str(&format!(
                            r#"<div class="formula"><img src="{}.png"><div class="label">{}</div><div class="latex">{}</div></div>"#,
                            name, name, latex.replace('<', "&lt;").replace('>', "&gt;")
                        ));
                    ok_count += 1;
                },
                Err(e) => {
                    html.push_str(&format!(
                            r#"<div class="formula"><div class="error">RENDER: {}</div><div class="label">{}</div></div>"#,
                            e, name
                        ));
                    errors.push(format!("RENDER {}: {}", name, e));
                    err_count += 1;
                },
            },
            Err(e) => {
                html.push_str(&format!(
                    r#"<div class="formula"><div class="error">PARSE: {}</div><div class="label">{}</div></div>"#,
                    e, name
                ));
                errors.push(format!("PARSE {}: {}", name, e));
                err_count += 1;
            },
        }
    }

    html.push_str("</body></html>");
    std::fs::write("test_gallery/audit/index.html", &html).expect("write html");

    println!("\n=== Rendering Audit Results ===");
    println!("OK: {}", ok_count);
    println!("Errors: {}", err_count);
    for e in &errors {
        println!("  - {}", e);
    }
    println!("\nAudit page: test_gallery/audit/index.html");
}
