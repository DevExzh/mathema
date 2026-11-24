//! Hand-written UnicodeMath with bars: which open, close, divide, or are
//! script bases (Unicode Technical Note 28 section 3.1), seen in what the
//! fractions around them take.

use mathema::ast::normalize::normalize_expr;
use mathema::error::Config;
use mathema::reader::{
    Reader, latex::LatexReader, mathml::MathmlReader, unicode_math::UnicodeMathReader,
};
use mathema::writer::{
    Writer, latex::LatexWriter, mathml::MathmlWriter, unicode_math::UnicodeMathWriter,
};
use mathema::{Format, convert, from_latex, from_mathml, from_unicode_math};

/// UnicodeMath and its LaTeX.
#[rustfmt::skip]
const BARS: &[(&str, &str)] = &[
    ("f|_0^1/2", r"\frac{f|_{0}^{1}}{2}"),
    ("f|_(x=0)/g", r"\frac{f|_{x= 0}}{g}"),
    ("2f|_0/g", r"\frac{2f|_{0}}{g}"),
    ("(f|_0)/2", r"\frac{f|_{0}}{2}"),
    ("f(x)|_0/2", r"\frac{f\left( x \right)|_{0}}{2}"),
    ("f(x)|_(x=0)/g(x)|_(x=0)", r"\frac{f\left( x \right)|_{x= 0}}{g\left( x \right)}|_{x= 0}"),
    ("[f(x)|_0/2]", r"\left[ \frac{f\left( x \right)|_{0}}{2} \right]"),
    ("F|_0+|x/y|", r"F|_{0}+ |\frac{x}{y}|"),
    ("F|_0^1=|a/b|", r"F|_{0}^{1}= |\frac{a}{b}|"),
    ("F|_0 |x/y|", r"F|_{0}|\frac{x}{y}|"),
    ("F|_0^1 |a/b|+c/d", r"F|_{0}^{1}|\frac{a}{b}|+ \frac{c}{d}"),
    ("x|_a-y|_b/c", r"x|_a- \frac{y|_b}{c}"),
    ("x|_a y|_b/c", r"x|_a\frac{y|_b}{c}"),
    ("a+f|_0 g/h", r"a+ f|_{0}\frac{g}{h}"),
    ("∂f/∂x|_0/y", r"\frac{\frac{\partial f}{\partial x}|_{0}}{y}"),
    ("v|_i^j w/z", r"v|_i^j\frac{w}{z}"),
    ("R|_A/S", r"\frac{R|_A}{S}"),
    ("df/dx|_(x=0)/2", r"\frac{\frac{df}{dx}|_{x= 0}}{2}"),
    ("||x/y|-z|", r"||\frac{x}{y}|- z|"),
    ("||x|-y/z|", r"||x|- \frac{y}{z}|"),
    ("|1-|x/y||", r"|1- |\frac{x}{y}||"),
    ("||x|/2|", r"|\frac{|x|}{2}|"),
    ("||x||/2", r"\frac{||x||}{2}"),
    ("||a/b|/c|", r"|\frac{|\frac{a}{b}|}{c}|"),
    ("|x+|y/z||", r"|x+ |\frac{y}{z}||"),
    ("|a+|b|/c|", r"|a+ \frac{|b|}{c}|"),
    ("{x∈A| |x/y|<1}", r"\left\{ x\in A||\frac{x}{y}|<1 \right\}"),
    ("{x∈A||x/y|<1}", r"\left\{ x\in A||\frac{x}{y}|<1 \right\}"),
    ("{x||x|/2>0}", r"\left\{ x|\frac{|x|}{2}>0 \right\}"),
    ("{x | |x|/2>0}", r"\left\{ x|\frac{|x|}{2}>0 \right\}"),
    ("{x : |x/y|>0}", r"\left\{ x:|\frac{x}{y}|>0 \right\}"),
    ("(a|b|c/d)", r"\left( \frac{a|b|c}{d} \right)"),
    ("(x| |y|/z)", r"\left( x|\frac{|y|}{z} \right)"),
    ("(x||y|/z)", r"\left( x|\frac{|y|}{z} \right)"),
    ("P(A|B,C)/D", r"\frac{P\left( A|B,C \right)}{D}"),
    ("P(A|B)/P(A|C)", r"\frac{P\left( A|B \right)}{P\left( A|C \right)}"),
    ("P(A|B,C|D/E)", r"P\left( A|B,C|\frac{D}{E} \right)"),
    ("E[|X/Y| | Z]", r"E\left[ |\frac{X}{Y}||Z \right]"),
    ("E[|X| |Y]/Z", r"\frac{E\left[ |X||Y \right]}{Z}"),
    ("d|n/d", r"d|\frac{n}{d}"),
    ("∑_(d|n) μ(d)/d", r"\sum\limits_{d|n} {\frac{\mu\left( d \right)}{d}}"),
    ("∑_(d|n) d/n", r"\sum\limits_{d|n} {\frac{d}{n}}"),
    ("a|b,c|d/e", r"a|b,c|\frac{d}{e}"),
    ("a|b∧b|c/d", r"a|b\wedge b|\frac{c}{d}"),
    ("a|b;c/d", r"a|b;\frac{c}{d}"),
    ("a|b+c/d", r"a|b+ \frac{c}{d}"),
    ("a|b=c/d", r"a|b= \frac{c}{d}"),
    ("a|b=c|d/e", r"a|b= c|\frac{d}{e}"),
    ("p|ab⇒p|a/b", r"p|ab\Rightarrow p|\frac{a}{b}"),
    ("1/2|x|/y", r"\frac{\frac{1}{2}|x|}{y}"),
    ("a|x/y|b/c", r"\frac{a|\frac{x}{y}|b}{c}"),
    ("|x/y|^2/z", r"\frac{|\frac{x}{y}|^{2}}{z}"),
    ("|x/y|z/w", r"\frac{|\frac{x}{y}|z}{w}"),
    ("|x/y|/z", r"\frac{|\frac{x}{y}|}{z}"),
    ("|x/y|/|z/w|", r"\frac{|\frac{x}{y}|}{|\frac{z}{w}|}"),
    ("|x/y|+|z/w|", r"|\frac{x}{y}|+ |\frac{z}{w}|"),
    ("|x/y|z", r"|\frac{x}{y}|z"),
    ("‖x/y‖^2", r"\|\frac{x}{y}{\|}^{2}"),
    ("‖x/y‖/z", r"\frac{\|\frac{x}{y}\|}{z}"),
    ("‖|x/y|‖", r"\||\frac{x}{y}|\|"),
    ("|‖x/y‖|", r"|\|\frac{x}{y}\||"),
    ("‖x‖|y/z|", r"\|x\||\frac{y}{z}|"),
    ("‖a/b‖‖c/d‖", r"\|\frac{a}{b}\|\|\frac{c}{d}\|"),
    ("⟨x|y/z⟩", r"\langle x|\frac{y}{z}\rangle"),
    ("⟨x/y|z⟩", r"\langle\frac{x}{y}|z\rangle"),
    ("⟨ψ|A/B|φ⟩", r"\langle\psi|\frac{A}{B}|\varphi\rangle"),
    ("⟨a|b⟩/⟨c|d⟩", r"\frac{\langle a|b\rangle}{\langle c|d\rangle}"),
    ("|ψ⟩/2", r"\frac{|\psi\rangle}{2}"),
    ("|ψ⟩⟨φ|/2", r"\frac{|\psi\rangle\langle\varphi|}{2}"),
    ("|0⟩+|1⟩/√2", r"|0\rangle+ \frac{|1\rangle}{\sqrt{2}}"),
    ("(|0⟩+|1⟩)/√2", r"\frac{|0\rangle+ |1\rangle}{\sqrt{2}}"),
    ("1/√2 (|0⟩+|1⟩)", r"\frac{1}{\sqrt{2}}\left( |0\rangle+|1\rangle \right)"),
    ("1/√2(|0⟩+|1⟩)", r"\frac{1}{\sqrt{2}\left( |0\rangle+|1\rangle \right)}"),
    ("1/√2|0⟩", r"\frac{1}{\sqrt{2}}|0\rangle"),
    ("|0⟩/√2+|1⟩/√2", r"\frac{|0\rangle}{\sqrt{2}}+ \frac{|1\rangle}{\sqrt{2}}"),
    ("|↑⟩/a", r"\frac{|\uparrow \rangle}{a}"),
    ("⟨x|/a", r"\frac{\langle x|}{a}"),
    ("⟨x|y|/z⟩", r"\langle\frac{x|y|}{z}\rangle"),
    ("|a/b⟩", r"|\frac{a}{b}\rangle"),
    ("⟨a/b|", r"\langle\frac{a}{b}|"),
    ("|n⟩ 1/2 ⟨n|", r"|n\rangle\frac{1}{2}\langle n|"),
    ("H|ψ⟩=E/2|ψ⟩", r"H|\psi\rangle= \frac{E}{2}|\psi\rangle"),
    ("iℏ ∂/∂t|ψ⟩=H|ψ⟩", r"i\hbar\frac{\partial}{\partial t}|\psi\rangle= H|\psi\rangle"),
    ("Tr(|a⟩⟨a|/2)", r"Tr\left( \frac{|a\rangle\langle a|}{2} \right)"),
    ("∑_n|n⟩⟨n|/2", r"\sum\limits_n {\frac{|n\rangle\langle n|}{2}}"),
    ("∑_n |n/m|", r"\sum\limits_n {|\frac{n}{m}|}"),
    ("∏|a/b|^2", r"\prod\limits {|\frac{a}{b}|^{2}}"),
    ("∫|x/y|", r"\int\nolimits {|\frac{x}{y}|}"),
    ("√(|x/y|)", r"\sqrt{|\frac{x}{y}|}"),
    ("√(n&|x/y|)", r"\sqrt[n]{|\frac{x}{y}|}"),
    ("x_(|a/b|)", r"x_{|\frac{a}{b}|}"),
    ("x^(|a|b/c)", r"x^{\frac{|a|b}{c}}"),
    ("|x|^(1/2)", r"|x|^{\frac{1}{2}}"),
    ("|x_1/x_2|", r"|\frac{x_{1}}{x_{2}}|"),
    ("|■(a&b@c&d)|/2", r"\frac{|\begin{matrix} a & b \\ c & d \end{matrix}|}{2}"),
    ("1/|■(a&b@c&d)|", r"\frac{1}{|\begin{matrix} a & b \\ c & d \end{matrix}|}"),
    ("a||b/c", r"a||\frac{b}{c}"),
    ("a∥b/c", r"a\parallel \frac{b}{c}"),
    ("a∣b/c", r"a\mid \frac{b}{c}"),
    ("⌊|x|/2⌋", r"\lfloor\frac{|x|}{2}\rfloor"),
    ("⌈|x/y|⌉", r"\lceil|\frac{x}{y}|\rceil"),
    ("|⌊x/y⌋|", r"|\lfloor\frac{x}{y}\rfloor|"),
    ("|(a+b)/c|", r"|\frac{a+ b}{c}|"),
    ("|(a)/b|", r"|\frac{a}{b}|"),
    ("|[a]/b|", r"|\frac{\left[ a \right]}{b}|"),
    ("|a b/c|", r"|a\frac{b}{c}|"),
    ("|a+b c/d|", r"|a+ b\frac{c}{d}|"),
    ("|-a/b|", r"|- \frac{a}{b}|"),
    ("|+a/b|", r"|+ \frac{a}{b}|"),
    ("|√x/y|", r"|\frac{\sqrt{x}}{y}|"),
    ("|sin x/y|", r"|\sin \frac{x}{y}|"),
    ("|x̂/y|", r"|\frac{\hat{x}}{y}|"),
    ("|x_1^2/y|", r"|\frac{x_{1}^{2}}{y}|"),
    ("|1/x|", r"|\frac{1}{x}|"),
    ("|1/x|y", r"|\frac{1}{x}|y"),
    ("| 1/x |y", r"|\frac{1}{x}|y"),
    ("|x/y |z", r"|\frac{x}{y}|z"),
    ("||/2", r"\frac{||}{2}"),
    ("|/2", r"\frac{|}{2}"),
    ("a|/2", r"\frac{a|}{2}"),
    ("|", "|"),
    ("‖", r"\|"),
    ("|a", "|a"),
    ("|a/b", r"|\frac{a}{b}"),
    ("a/b|", r"\frac{a}{b}|"),
    ("a/b|c", r"\frac{a}{b}|c"),
    ("a/|b", r"\frac{a}{|b}"),
    ("a/|", r"\frac{a}{|}"),
    ("a|/b", r"\frac{a|}{b}"),
    ("x|y|/z|w|", r"\frac{x|y|}{z}|w|"),
    ("max|x/y|", r"\max |\frac{x}{y}|"),
    ("sup_x|f(x)/g(x)|", r"\sup_x|\frac{f\left( x \right)}{g\left( x \right)}|"),
    ("lim_(x→0)|sin x/x|", r"\lim_{x\rightarrow 0}|\sin \frac{x}{x}|"),
    ("lim|a/b|", r"\lim |\frac{a}{b}|"),
    ("log|x/y|", r"\log |\frac{x}{y}|"),
    ("ln|x|/2", r"\frac{\ln |x|}{2}"),
    ("sin|x/y|", r"\sin |\frac{x}{y}|"),
    ("|z|=r/s", r"|z|= \frac{r}{s}"),
    ("|z|e^iθ/2", r"\frac{|z|e^{i\theta}}{2}"),
    ("e^|x|/2", r"\frac{e^{|x|}}{2}"),
    ("e^(-|x|/2)", r"e^{- \frac{|x|}{2}}"),
    ("e^(-|x/y|)", r"e^{- |\frac{x}{y}|}"),
    ("2^(|A|/2)", r"2^{\frac{|A|}{2}}"),
    ("|A|!/|B|!", r"\frac{|A|!}{|B|!}"),
    ("|G:H|=|G|/|H|", r"|G: H|= \frac{|G|}{|H|}"),
    ("[G:H]=|G|/|H|", r"\left[ G:H \right]= \frac{|G|}{|H|}"),
    ("|G|/|H|=n", r"\frac{|G|}{|H|}= n"),
    ("|a|⋅|b|/c", r"|a|\cdot \frac{|b|}{c}"),
    ("|a|×|b/c|", r"|a|\times |\frac{b}{c}|"),
    ("cosθ=(a⋅b)/(|a||b|)", r"cos\theta= \frac{a\cdot b}{|a||b|}"),
    ("cosθ=(a⋅b)/(|a| |b|)", r"cos\theta= \frac{a\cdot b}{|a||b|}"),
    ("(a⋅b)/|a||b|", r"\frac{a\cdot b}{|a|}|b|"),
    ("|a⋅b|/|a||b|", r"\frac{|a\cdot b|}{|a|}|b|"),
    ("|a×b|/2", r"\frac{|a\times b|}{2}"),
    ("d=|ax+by+c|/√(a^2+b^2)", r"d= \frac{|ax+ by+ c|}{\sqrt{a^{2}+ b^{2}}}"),
    ("|x-μ|/σ", r"\frac{|x- \mu|}{\sigma}"),
    ("e^(-|x-μ|/b)/2b", r"\frac{e^{- \frac{|x- \mu|}{b}}}{2b}"),
    ("1/2b e^(-|x-μ|/b)", r"\frac{1}{2b}e^{- \frac{|x- \mu|}{b}}"),
    ("|x_n-x|<1/n", r"|x_n- x|< \frac{1}{n}"),
    ("0<|x-a|<δ/2", r"0< |x- a|< \frac{\delta}{2}"),
    ("|f(x)-L|<ε/2", r"|f\left( x \right)- L|< \frac{\varepsilon}{2}"),
    ("|∑_i a_i/b_i|", r"|\sum\limits_i {\frac{a_i}{b_i}}|"),
    ("|∫f/g|", r"|\int\nolimits {\frac{f}{g}}|"),
    ("|∫_0^1▒f/g|", r"|\int\nolimits_{0}^{1} {\frac{f}{g}}|"),
    ("|∑_i a_i|/n", r"\frac{|\sum\limits_i {a_i}|}{n}"),
    ("‖x-y‖≤‖x‖/2", r"\|x- y\|\leq \frac{\|x\|}{2}"),
    ("‖A‖=max‖Ax‖/‖x‖", r"\|A\|= \frac{\max \|Ax\|}{\|x\|}"),
    ("∑_i |x_i-y_i|/n", r"\sum\limits_i {\frac{|x_i- y_i|}{n}}"),
    ("∑|a+b|/c", r"\sum\limits {\frac{|a+ b|}{c}}"),
    ("∫|f+g|/h", r"\int\nolimits {\frac{|f+ g|}{h}}"),
    ("∫_0^1 |x-1| ⅆx/2", r"\int\nolimits_{0}^{1} {|x- 1|\frac{ⅆx}{2}}"),
    ("∑|a+b||c/d|", r"\sum\limits {|a+ b||\frac{c}{d}|}"),
    ("max |a-b|/c", r"\max \frac{|a- b|}{c}"),
    ("sin |a-b|/c", r"\sin \frac{|a- b|}{c}"),
    ("1/|a+b|+c|d|/e", r"\frac{1}{|a+ b|}+ \frac{c|d|}{e}"),
    ("1/|x|+|y|/2", r"\frac{1}{|x|}+ \frac{|y|}{2}"),
    ("1/|x|+a|y|/2", r"\frac{1}{|x|}+ \frac{a|y|}{2}"),
    ("a/|b|+|c/d|", r"\frac{a}{|b|}+ |\frac{c}{d}|"),
    ("x_|a|+|c/d|", r"x_{|a|}+ |\frac{c}{d}|"),
    ("(A|B,C|D/E)", r"\left( A|B,C|\frac{D}{E} \right)"),
];

fn check(rows: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for &(input, latex) in rows {
        let wanted = structure(&convert(latex, Format::LaTeX, Format::MathML).unwrap());
        for (path, written) in by_path(input, Format::UnicodeMath, Format::LaTeX) {
            if path == "convert" && written != latex {
                failures.push(format!("{input}: found {written}, wanted {latex}"));
            }
        }
        for (path, mathml) in by_path(input, Format::UnicodeMath, Format::MathML) {
            let found = structure(&mathml);
            if found != wanted {
                failures.push(format!("{path} {input}: found {found}, wanted {wanted}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Hand-written expectations (UnicodeMath, LaTeX with the same structure).
#[rustfmt::skip]
const EXPECTED: &[(&str, &str)] = &[
    ("⌊x⌋y/2", r"\frac{\lfloor x\rfloor y}{2}"),
    ("⌊x⌋²y/2", r"\frac{\lfloor x\rfloor^2y}{2}"),
    ("2⌊x⌋/3", r"\frac{2\lfloor x\rfloor}{3}"),
    ("a+⌈x/y⌉z/2", r"a+\frac{\lceil\frac{x}{y}\rceil z}{2}"),
    ("f⟨x⟩/2", r"\frac{f\langle x\rangle}{2}"),
    ("|x/2|=|y/3|", r"|\frac{x}{2}|=|\frac{y}{3}|"),
    ("|a/b|<|c/d|", r"|\frac{a}{b}|<|\frac{c}{d}|"),
    ("|x/2|,|y/3|", r"|\frac{x}{2}|,|\frac{y}{3}|"),
    ("{x | |x/2|<1}", r"\{x||\frac{x}{2}|<1\}"),
    ("{x||x/2|<1}", r"\{x||\frac{x}{2}|<1\}"),
    ("|a/b|∧|b/c|", r"|\frac{a}{b}|\wedge|\frac{b}{c}|"),
    ("P(A|B)=P(A∩B)/P(B)", r"P(A|B)=\frac{P(A\cap B)}{P(B)}"),
    ("⟨a|b⟩/c", r"\frac{\langle a|b\rangle}{c}"),
    ("⟨a/b|c/d⟩", r"\langle\frac{a}{b}|\frac{c}{d}\rangle"),
    ("‖x/2‖≤‖y‖/2", r"\|\frac{x}{2}\|\le\frac{\|y\|}{2}"),
    ("|x+y/2|≤|x/2|+|y|/2", r"|x+\frac{y}{2}|\le|\frac{x}{2}|+\frac{|y|}{2}"),
    ("||x/2||", r"||\frac{x}{2}||"),
    ("P(A|B,C|D/E)", r"P(A|B,C|\frac{D}{E})"),
    ("P(A|B/C)", r"P(A|\frac{B}{C})"),
    ("{x|x/2∈Z}", r"\{x|\frac{x}{2}\in Z\}"),
    ("D(p‖q/r)", r"D(p\|\frac{q}{r})"),
    ("a|b/c", r"a|\frac{b}{c}"),
    ("|a=b|c/d", r"\frac{|a=b|c}{d}"),
    ("x|a=b|c/d", r"x|a=b|\frac{c}{d}"),
    ("2|a-b|c/d", r"\frac{2|a-b|c}{d}"),
    ("2|a<b|/c", r"\frac{2|a<b|}{c}"),
    ("c|x,y|/2", r"\frac{c|x,y|}{2}"),
    ("|⟨x,y⟩|/2", r"\frac{|\langle x,y\rangle|}{2}"),
    ("|⟨x,y⟩|≤‖x‖‖y‖/2", r"|\langle x,y\rangle|\le\frac{\|x\|\|y\|}{2}"),
    ("|⟨x|y⟩|^2/2", r"\frac{|\langle x|y\rangle|^2}{2}"),
    ("|⟨x/a|y⟩|", r"|\langle\frac{x}{a}|y\rangle|"),
    ("|{x|x>0}|/n", r"\frac{|\{x|x>0\}|}{n}"),
    ("|{x∣x>0}|/n", r"\frac{|\{x\mid x>0\}|}{n}"),
    ("|A∩B|/|A∪B|", r"\frac{|A\cap B|}{|A\cup B|}"),
    ("|A|/|B|", r"\frac{|A|}{|B|}"),
    ("√|x|", r"\sqrt{|x|}"),
    ("1/|x+y|", r"\frac{1}{|x+y|}"),
    ("∑|a+b|", r"\sum |a+b|"),
    ("∑|a+b|/c", r"\sum\frac{|a+b|}{c}"),
    ("∑_i|x_i-y_i|/n", r"\sum_i\frac{|x_i-y_i|}{n}"),
    ("|∑_i x_i/n|", r"|\sum_i\frac{x_i}{n}|"),
    ("‖x‖_2/n", r"\frac{\|x\|_2}{n}"),
    ("|x|^2/|y|^2", r"\frac{|x|^2}{|y|^2}"),
    ("|a→b|/c", r"\frac{|a\to b|}{c}"),
    ("f(x)|_(x=0)^(x=1)/2", r"\frac{f(x)|_{x=0}^{x=1}}{2}"),
    ("(f|_0)/(g|_1)", r"\frac{f|_0}{g|_1}"),
    ("|f|_0/2", r"\frac{|f|_0}{2}"),
    ("|f|_0|^2/2", r"\frac{|f|_0|^2}{2}"),
    ("A|ψ⟩/2", r"\frac{A|\psi\rangle}{2}"),
    ("⟨ψ|A|ψ⟩/⟨ψ|ψ⟩", r"\frac{\langle\psi|A|\psi\rangle}{\langle\psi|\psi\rangle}"),
    ("∑_i p_i|i⟩⟨i|/2", r"\sum_i\frac{p_i|i\rangle\langle i|}{2}"),
    ("(|00⟩+|11⟩)/√2", r"\frac{|00\rangle+|11\rangle}{\sqrt2}"),
    ("|0⟩⊗|1⟩/2", r"|0\rangle\otimes\frac{|1\rangle}{2}"),
    ("|a+b⟩/2", r"\frac{|a+b\rangle}{2}"),
    ("⟨x,y⟩/2", r"\frac{\langle x,y\rangle}{2}"),
    ("⟨x+y⟩/2", r"\frac{\langle x+y\rangle}{2}"),
    ("⟨x^2⟩-⟨x⟩^2/2", r"\langle x^2\rangle-\frac{\langle x\rangle^2}{2}"),
    ("sin|x/2|", r"\sin|\frac{x}{2}|"),
    ("|x|!/2", r"\frac{|x|!}{2}"),
    ("|x| y/z", r"|x|\frac{y}{z}"),
    ("-|x|/y", r"-\frac{|x|}{y}"),
    ("⟨|x/y|⟩", r"\langle|\frac{x}{y}|\rangle"),
    ("[|x/y|,|z|/w]", r"[|\frac{x}{y}|,\frac{|z|}{w}]"),
    ("max(|x/a|,|y/b|)", r"\max(|\frac{x}{a}|,|\frac{y}{b}|)"),
    ("|x/a|+|y/b|=1", r"|\frac{x}{a}|+|\frac{y}{b}|=1"),
    ("|x-y|/|z-w|", r"\frac{|x-y|}{|z-w|}"),
    ("1/2|a|+b/c", r"\frac12|a|+\frac{b}{c}"),
    ("d|a⇒d|b/c", r"d|a\Rightarrow d|\frac{b}{c}"),
    ("I(X;Y|Z)/2", r"\frac{I(X;Y|Z)}{2}"),
    ("H(X|Y=y/2)", r"H(X|Y=\frac{y}{2})"),
    ("|x|≥0∧|y/2|>1", r"|x|\ge0\wedge|\frac{y}{2}|>1"),
    ("∀x|f(x)/2|<1", r"\forall x|\frac{f(x)}{2}|<1"),
    ("|A ∪ B| /2", r"|A\cup B|/2"),
    ("|x/y | + z", r"|\frac{x}{y}|+z"),
    ("1/|x|", r"\frac{1}{|x|}"),
    ("√|x|^2", r"\sqrt{|x|}^2"),
    ("√|x+y|/2", r"\frac{\sqrt{|x+y|}}{2}"),
    ("x_|a|", "x_{|a|}"),
    ("x_|a|^2", "x_{|a|}^2"),
    ("e^|x|/2", r"\frac{e^{|x|}}{2}"),
    ("e^-|x|", "e^{-|x|}"),
    ("e^‖x‖", r"e^{\|x\|}"),
    ("1/⌊x⌋", r"\frac{1}{\lfloor x\rfloor}"),
    ("1/⟨x,y⟩", r"\frac{1}{\langle x,y\rangle}"),
    ("√⌊x/2⌋", r"\sqrt{\lfloor\frac{x}{2}\rfloor}"),
    ("∑|a| b", r"\sum|a| b"),
    ("∑|a|+b", r"\sum|a|+b"),
    ("|∑ a|", r"|\sum a|"),
    ("|∑_i a_i|/n", r"\frac{|\sum_i a_i|}{n}"),
    ("∑ p|x|/2", r"\sum\frac{p|x|}{2}"),
    ("∫_0^1 |x-1| ⅆx", r"\int_0^1|x-1|ⅆx"),
    ("|x|/|y|/|z|", r"\frac{\frac{|x|}{|y|}}{|z|}"),
    ("a/|b/c|", r"\frac{a}{|\frac{b}{c}|}"),
    ("1/|x y|", r"\frac{1}{|xy|}"),
    ("1/|√|x||", r"\frac{1}{|\sqrt{|x|}|}"),
    ("1/|ψ⟩", r"\frac{1}{|\psi\rangle}"),
    ("⟨ψ|φ⟩/⟨ψ|ψ⟩", r"\frac{\langle\psi|\varphi\rangle}{\langle\psi|\psi\rangle}"),
    ("|x|/|y| z", r"\frac{|x|}{|y|}z"),
    ("|x|/|y|z", r"\frac{|x|}{|y|z}"),
    ("|x|/|y|+z", r"\frac{|x|}{|y|}+z"),
    ("∬|f|^2/g", r"\iint\frac{|f|^2}{g}"),
    ("max_x|f(x)|/g", r"\frac{\max_x|f(x)|}{g}"),
];

/// Section 3.12 script characters, 3.10 apostrophes, 3.5 function apply, 3.6 radicands.
#[rustfmt::skip]
const SCRIPTS: &[(&str, &str)] = &[
    ("x^-½", r"x^{-½}"),
    ("e^-½x²", r"e^{-½x^2}"),
    ("x²'", r"x^{2\prime}"),
    ("x^2'", r"x^{2\prime}"),
    ("x^2′", r"x^{2\prime}"),
    ("x_1²'/2", r"\frac{x_1^{2\prime}}{2}"),
    ("∑ₖ₌₀ⁿ xₖ", r"\sum_{k=0}^n x_k"),
    ("xᵢⱼ", r"x_{ij}"),
    ("Aᵀ/b", r"\frac{A^T}{b}"),
    ("sin\u{2061} \u{2061}^n x", r"\sin^n x"),
    ("∫₁ max \u{2061}_2‖x‖", r"\int_1\max_2\|x\|"),
    ("x²", "x^2"),
    ("x²/2", r"\frac{x^2}{2}"),
    ("x₁²/y₁²", r"\frac{x_1^2}{y_1^2}"),
    ("α₂³/(β₂³ + γ₂³)", r"\frac{\alpha_2^3}{\beta_2^3+\gamma_2^3}"),
    ("W^3β_δ₁ρ₁σ₂", r"W^{3\beta}_{\delta_1\rho_1\sigma_2}"),
    ("a +₁ b =₂ c", "a+_1 b=_2 c"),
    ("x⁻¹", "x^{-1}"),
    ("10⁻³", "10^{-3}"),
    ("xⁿ⁺¹", "x^{n+1}"),
    ("e^x²", "e^{x^2}"),
    ("x_i²", "x_i^2"),
    ("x^2₁", "x_1^2"),
    ("(a+b)²/c", r"\frac{{(a+b)}^2}{c}"),
    ("sin²x", r"\sin^2x"),
    ("sin² x", r"\sin^2x"),
    ("½", "½"),
    ("½x", "½x"),
    ("½/a", r"\frac{½}{a}"),
    ("f'", "f'"),
    ("f'(x)/2", r"\frac{f'(x)}{2}"),
    ("f''", "f''"),
    ("f'^2", "f'^2"),
    ("a'^c", "a'^c"),
    ("f'_1", "f'_1"),
    ("sin⁡x", r"\sin x"),
    ("sin⁡(x) b", r"\sin(x)b"),
    ("sin⁡^2 x", r"\sin^2x"),
    ("f⁡(x)", "f(x)"),
    ("sin⁡x/y", r"\sin\frac{x}{y}"),
    ("√abc", r"\sqrt{abc}"),
    ("√2x", r"\sqrt{2x}"),
    ("√x+1", r"\sqrt{x}+1"),
    ("√ab/c", r"\frac{\sqrt{ab}}{c}"),
    ("√a b", r"\sqrt{a}b"),
    ("√x^2", r"\sqrt{x}^2"),
    ("√x_1", r"\sqrt{x}_1"),
    ("∛abc", r"\sqrt[3]{abc}"),
    ("√2π", r"\sqrt{2\pi}"),
    ("1/√2π", r"\frac{1}{\sqrt{2\pi}}"),
    ("√max", r"\sqrt{max}"),
    ("|x|²/2", r"\frac{|x|^2}{2}"),
    ("x²y/z", r"\frac{x^2y}{z}"),
    ("∑₁² a", r"\sum_1^2a"),
    ("∑ⁿ_i a", r"\sum_i^na"),
    ("A²⁺", "A^{2+}"),
    ("H₂O", "H_2O"),
];

/// `input` in another format by the four paths: `convert`, the builder, and the
/// heap AST as read and normalized.
fn by_path(input: &str, from: Format, to: Format) -> [(&'static str, String); 4] {
    let config = Config::default();
    let write = |expr: &mathema::ast::MathExpr| {
        match to {
            Format::LaTeX => LatexWriter::write(expr, &config),
            Format::UnicodeMath => UnicodeMathWriter::write(expr, &config),
            _ => MathmlWriter::write(expr, &config),
        }
        .unwrap()
    };
    let read = match from {
        Format::LaTeX => LatexReader::parse(input, &config),
        Format::UnicodeMath => UnicodeMathReader::parse(input, &config),
        _ => MathmlReader::parse(input, &config),
    };
    let mut expr = read.unwrap().expr;
    let heap = write(&expr);
    normalize_expr(&mut expr);
    let built = match from {
        Format::LaTeX => from_latex(input),
        Format::UnicodeMath => from_unicode_math(input),
        _ => from_mathml(input),
    }
    .unwrap();
    let built = match to {
        Format::LaTeX => built.to_latex(),
        Format::UnicodeMath => built.to_unicode_math(),
        _ => built.to_mathml(),
    };
    [
        ("convert", convert(input, from, to).unwrap()),
        ("builder", built.unwrap()),
        ("heap", heap),
        ("normalized heap", write(&expr)),
    ]
}

/// MathML with leaves joined and rows spliced: what is in which argument.
fn structure(mathml: &str) -> String {
    let mut out = String::new();
    let mut open: Vec<&str> = Vec::new();
    for (tag, text) in mathml.split('<').skip(1).filter_map(|t| t.split_once('>')) {
        let name = tag
            .trim_start_matches('/')
            .split([' ', '/'])
            .next()
            .unwrap();
        let kept = match name {
            "mi" | "mn" | "mo" | "mtext" | "math" | "mrow" | "mspace" | "mstyle" => "",
            "munder" => "msub",
            "mover" => "msup",
            "munderover" => "msubsup",
            name => name,
        };
        if !tag.starts_with('/') {
            open.push(kept);
            out.push_str(kept);
            if !kept.is_empty() {
                out.push('(');
            }
            out.push_str(&text.replace('\u{2212}', "-"));
        }
        if tag.starts_with('/') || tag.ends_with('/') {
            open.pop();
            if !kept.is_empty() {
                out.push(')');
            }
            // The end of an argument.
            if open.last().is_some_and(|parent| !parent.is_empty()) {
                out.push(';');
            }
        }
    }
    out
}

fn check_structure(rows: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for &(input, latex) in rows {
        let wanted = structure(&convert(latex, Format::LaTeX, Format::MathML).unwrap());
        for (path, mathml) in by_path(input, Format::UnicodeMath, Format::MathML) {
            let found = structure(&mathml);
            if found != wanted {
                failures.push(format!("{path} {input}: found {found}, wanted {wanted}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Decimals, points, grouping commas and units: LaTeX or MathML, which must
/// keep their structure through UnicodeMath, and the UnicodeMath.
#[rustfmt::skip]
const DECIMALS: &[(&str, &str)] = &[
    (".5", ".5"),
    ("x=.25", "x=.25"),
    ("(0,.5)", "(0,.5)"),
    (r"\frac{.5}{2}", "(.5)/2"),
    ("x^{.5}", "x^(.5)"),
    (r"1.3 \times .2", "1.3×.2"),
    (",5", ",5"),
    (".5.5", ".5.5"),
    ("1..5", "1..5"),
    ("5.", "5."),
    ("5.x", "5.x"),
    ("a.b", "a.b"),
    ("..", ".."),
    ("...", "..."),
    (r"\ldots 5", "…5"),
    (r"5\ldots", "5…"),
    ("1,234", "1,234"),
    ("1,234.5", "1,234.5"),
    ("x_{1.5}", "x_1.5"),
    (r"3.14\,\mathrm{m}", "3.14\u{2009}m"),
    (r"5\times10^{-3}", "5×10^−3"),
    (r"\sqrt{.5}", "√(.5)"),
    (r"\sum .5", "∑ .5"),
    (r"\sqrt2.5", "√2 .5"),
    ("x^2.5", "x^2 .5"),
    ("x^2.", "x^2 ."),
    (r"\frac12.5", "1/2 .5"),
    (r".\frac12", ". 1/2"),
    (r"2.\frac12", "2. 1/2"),
    (r"\frac{.}{x}", "(.)/x"),
    ("x.5", "x.5"),
    ("a.5.", "a.5."),
    ("|.5|", "|.5|"),
    (r"\{.5\}", "{.5}"),
    (r"1.\,5", "1.\u{2009}5"),
    ("<math><mo>.</mo><mn>5</mn></math>", ".5"),
    ("<math><mn>.5</mn></math>", ".5"),
    ("<math><mi>x</mi><mo>=</mo><mn>.5</mn></math>", "x=.5"),
    ("<math><mn>1</mn><mo>.</mo><mn>5</mn></math>", "1.5"),
    ("<math><mn>1,234.5</mn></math>", "1,234.5"),
];

#[test]
fn decimals_round_trip() {
    let mut failures = Vec::new();
    for &(input, unicode) in DECIMALS {
        let from = if input.starts_with('<') {
            Format::MathML
        } else {
            Format::LaTeX
        };
        let wanted = structure(&convert(input, from, Format::MathML).unwrap());
        for (path, written) in by_path(input, from, Format::UnicodeMath) {
            if written != unicode {
                failures.push(format!("{path} {input}: wrote {written}, wanted {unicode}"));
            }
            for (read, mathml) in by_path(&written, Format::UnicodeMath, Format::MathML) {
                if mathml.contains("mtext") || structure(&mathml) != wanted {
                    failures.push(format!("{path} {input}: {read} read {mathml}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn bars_read_as_expected() {
    check_structure(EXPECTED);
}

#[test]
fn script_characters_read_as_expected() {
    check_structure(SCRIPTS);
}

#[test]
fn bars_pair_as_section_3_1_says() {
    check(BARS);
}

#[test]
fn bar_scan_is_linear() {
    // Each would take minutes if a bar or bracket rescanned the others.
    for unit in [
        "|", "‖", "|x/y|", "⟨|", "|⟩", "⟨&", "‖⟩", "(|", "|x ", "⟨x| ", "a|b,", "‖|", "x|_0",
    ] {
        let input = unit.repeat(40_000 / unit.len());
        convert(&input, Format::UnicodeMath, Format::MathML).ok();
    }
}
