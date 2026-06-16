use mathema::from_latex;

fn main() {
    let test_cases = vec![
        // Basic math
        (r"a + b = c", "basic_add"),
        (r"x^2", "superscript"),
        (r"x_i", "subscript"),
        (r"x_i^2", "subsup"),
        (r"\frac{1}{2}", "frac"),
        (r"\sqrt{2}", "sqrt"),
        (r"\sqrt[3]{x}", "nth_root"),
        (r"\sum_{i=0}^n", "sum"),
        (r"\prod_{k=1}^n", "prod"),
        (r"\int_a^b", "int"),
        (r"\oint_C", "oint"),
        (r"\iint_D", "iint"),
        (r"\iiint_V", "iiint"),
        // Delimiters
        (r"\left( x \right)", "left_right_paren"),
        (r"\left[ x \right]", "left_right_bracket"),
        (r"\left\{ x \right\}", "left_right_brace"),
        (r"\left| x \right|", "left_right_abs"),
        (r"\left\| x \right\|", "left_right_norm"),
        (r"\left\langle x \right\rangle", "left_right_angle"),
        (r"\left\lfloor x \right\rfloor", "left_right_floor"),
        (r"\left\lceil x \right\rceil", "left_right_ceil"),
        (r"\left. x \right|", "left_right_dot"),
        // Text and fonts
        (r"\text{hello}", "text"),
        (r"\textbf{bold}", "textbf"),
        (r"\textit{italic}", "textit"),
        (r"\mathrm{abc}", "mathrm"),
        (r"\mathbf{abc}", "mathbf"),
        (r"\mathit{abc}", "mathit"),
        (r"\mathsf{abc}", "mathsf"),
        (r"\mathtt{abc}", "mathtt"),
        (r"\mathbb{R}", "mathbb"),
        (r"\mathcal{L}", "mathcal"),
        (r"\mathfrak{g}", "mathfrak"),
        (r"\mathscr{H}", "mathscr"),
        // Accents
        (r"\hat{a}", "hat"),
        (r"\bar{a}", "bar"),
        (r"\dot{a}", "dot"),
        (r"\ddot{a}", "ddot"),
        (r"\tilde{a}", "tilde"),
        (r"\vec{a}", "vec"),
        (r"\widehat{abc}", "widehat"),
        (r"\widetilde{abc}", "widetilde"),
        (r"\overline{abc}", "overline"),
        (r"\underline{abc}", "underline"),
        // Environments
        (r"\begin{matrix} a & b \\ c & d \end{matrix}", "matrix"),
        (r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", "pmatrix"),
        (r"\begin{bmatrix} a & b \\ c & d \end{bmatrix}", "bmatrix"),
        (r"\begin{vmatrix} a & b \\ c & d \end{vmatrix}", "vmatrix"),
        (r"\begin{Vmatrix} a & b \\ c & d \end{Vmatrix}", "Vmatrix"),
        (
            r"\begin{cases} x & x > 0 \\ -x & x \le 0 \end{cases}",
            "cases",
        ),
        (r"\begin{aligned} x &= 1 \\ y &= 2 \end{aligned}", "aligned"),
        (
            r"\begin{array}{|c|c|} a & b \\ c & d \end{array}",
            "array_rules",
        ),
        // Operators
        (r"\sin x", "sin"),
        (r"\cos \theta", "cos"),
        (r"\log n", "log"),
        (r"\lim_{x \to 0}", "lim"),
        (r"\max_{x}", "max"),
        (r"\inf S", "inf"),
        (r"\sup S", "sup"),
        (r"\det A", "det"),
        (r"\gcd(a,b)", "gcd"),
        // Special constructs
        (r"\binom{n}{k}", "binom"),
        (r"\dbinom{n}{k}", "dbinom"),
        (r"\overset{!}{=}", "overset"),
        (r"\underset{\sim}{A}", "underset"),
        (r"\stackrel{f}{\longrightarrow}", "stackrel"),
        (r"\xrightarrow{f}", "xrightarrow"),
        (r"\xleftarrow{g}", "xleftarrow"),
        // Decorations
        (r"\overbrace{abc}^n", "overbrace"),
        (r"\underbrace{abc}_n", "underbrace"),
        (r"\overrightarrow{AB}", "overrightarrow"),
        (r"\overleftarrow{AB}", "overleftarrow"),
        // Spacing
        (r"a\,b", "thinspace"),
        (r"a\;b", "thickspace"),
        (r"a\quad b", "quad"),
        (r"a\qquad b", "qquad"),
        (r"a\!b", "negspace"),
        // Colors
        (r"\textcolor{red}{x}", "textcolor"),
        (r"\color{blue} x + y", "color"),
        (r"\colorbox{yellow}{x}", "colorbox"),
        // Misc
        (r"\phantom{x}", "phantom"),
        (r"\vphantom{x}", "vphantom"),
        (r"\hphantom{x}", "hphantom"),
        (r"\cancel{x}", "cancel"),
        (r"\bcancel{x}", "bcancel"),
        (r"\xcancel{x}", "xcancel"),
        (r"\sout{x}", "sout"),
        (r"\boxed{x}", "boxed"),
        (r"\fbox{x}", "fbox"),
        // Greek
        (
            r"\alpha \beta \gamma \delta \epsilon \varepsilon",
            "greek_lower1",
        ),
        (r"\zeta \eta \theta \vartheta \iota \kappa", "greek_lower2"),
        (r"\lambda \mu \nu \xi \pi \varpi", "greek_lower3"),
        (
            r"\rho \varrho \sigma \varsigma \tau \upsilon",
            "greek_lower4",
        ),
        (r"\phi \varphi \chi \psi \omega", "greek_lower5"),
        (r"\Gamma \Delta \Theta \Lambda \Xi", "greek_upper1"),
        (r"\Pi \Sigma \Upsilon \Phi \Psi \Omega", "greek_upper2"),
        // Relations
        (r"a = b \neq c \approx d", "relations1"),
        (r"A \subset B \subseteq C \supset D", "relations2"),
        (r"a \le b \ge c \ll d \gg e", "relations3"),
        (r"a \in A \notin B \ni c", "relations4"),
        (r"a \equiv b \sim c \simeq d \cong e", "relations5"),
        (r"a \propto b \parallel c \perp d", "relations6"),
        // Binary ops
        (r"a \pm b \mp c \times d \div e", "binops1"),
        (r"A \cap B \cup C \setminus D", "binops2"),
        (r"a \cdot b \circ c \bullet d", "binops3"),
        (r"A \oplus B \otimes C \wedge D \vee E", "binops4"),
        // Arrows
        (r"\leftarrow \rightarrow \leftrightarrow", "arrows1"),
        (r"\Leftarrow \Rightarrow \Leftrightarrow", "arrows2"),
        (r"\uparrow \downarrow \updownarrow", "arrows3"),
        (r"\nearrow \searrow \swarrow \nwarrow", "arrows4"),
        (r"\hookrightarrow \hookleftarrow", "arrows5"),
        (r"\mapsto \longmapsto", "arrows6"),
        // Misc symbols
        (r"\infty \partial \nabla \emptyset", "misc1"),
        (r"\forall \exists \nexists \neg", "misc2"),
        (r"\hbar \ell \Re \Im", "misc3"),
        (r"\aleph \beth \daleth \gimel", "misc4"),
        (r"\ldots \cdots \vdots \ddots", "dots"),
        (r"a' a'' a'''", "primes"),
    ];

    let mut pass = 0;
    let mut fail = 0;
    let mut failures = Vec::new();

    for (latex, name) in &test_cases {
        match from_latex(latex) {
            Ok(result) => match result.to_svg() {
                Ok(svg) => {
                    if svg.contains("NaN") || svg.contains("Infinity") {
                        failures.push(format!("{}: NaN/Infinity in SVG", name));
                        fail += 1;
                    } else {
                        pass += 1;
                    }
                },
                Err(e) => {
                    failures.push(format!("{}: SVG error: {}", name, e));
                    fail += 1;
                },
            },
            Err(e) => {
                failures.push(format!("{}: parse error: {}", name, e));
                fail += 1;
            },
        }
    }

    println!("=== Coverage Test Results ===");
    println!("Pass: {}, Fail: {}, Total: {}", pass, fail, pass + fail);
    if !failures.is_empty() {
        println!("\nFailures:");
        for f in &failures {
            println!("  - {}", f);
        }
    } else {
        println!("\nAll {} test cases passed!", pass);
    }
}
