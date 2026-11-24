//! Which color specs LaTeX output can hold.

/// Whether `spec` may be written in a color argument: a name, `#hex`, a
/// numeric list, an `!` mix or another xcolor expression (`red!!+`,
/// `foo!![2]`, `red>wheel,1,12`). The argument is expanded and parsed by TeX,
/// so anything else (a command, `^^5c`, a parameter, a blank line) could do
/// more than name a color. What that drops is `\ { } % # $ & ^ ~`, control and
/// non-ASCII characters, `= @ < ? " |` and the backtick.
pub(crate) fn is_color(spec: &str) -> bool {
    if let Some(hex) = spec.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    !spec.is_empty()
        && spec
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!.,-:;() _/'+*>[]".contains(&b))
}

/// Whether `spec` is written as the color of `\textcolor`: without `_`, which
/// KaTeX refuses, and of the hexadecimal ones only `#rrggbb`, which has a
/// spelling in xcolor (`[HTML]`). KaTeX would refuse a formula that is fine
/// without its color.
pub(crate) fn is_text_color(spec: &str) -> bool {
    is_color(spec) && !spec.contains('_') && (!spec.starts_with('#') || spec.len() == 7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_allowlisted() {
        for color in [
            "red",
            "Dark Blue",
            "#f00",
            "#ff0000",
            "#FF000080",
            "red!50",
            "red!50!blue",
            "-red",
            ".",
            "0.5,0.5,0",
            "rgb(1,2,3)",
            "rgb:red,1;blue,2",
            "my_c",
            "red!!+",
            "foo!![2]",
            "red>wheel,1,12",
            "rgb:red!50/2+blue*3,x'",
        ] {
            assert!(is_color(color), "{color}");
        }
        for color in [
            "",
            r"\input{pwn}",
            r"\write18{ls}",
            "^^5cinput",
            "#1",
            "#12",
            "a#123",
            "#gggggg",
            "a\n\nb",
            "a\tb",
            "a%",
            "a~",
            "$x$",
            "a&b",
            "{red}",
            "}",
            "é",
            "\0",
            "a\u{A0}b",
            "a=b",
            "@x",
        ] {
            assert!(!is_color(color), "{color}");
        }
    }
}
