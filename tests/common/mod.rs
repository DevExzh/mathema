//! Helpers shared by integration tests.

/// Check TeX's one-token rule for every script marker in written LaTeX.
///
/// TeX, KaTeX and MathJax take exactly one token after `^` or `_` (spaces
/// before it are skipped), so `x^22` is `x^{2}2`. Every marker must therefore
/// be followed by a brace group, a control sequence that takes no group right
/// after it, or a single character. `\char` is not one token: it is followed
/// by its character code. A single digit must also not be followed by a digit
/// or `.`: a human reading the source takes the whole decimal as the
/// argument, while TeX takes the one digit.
///
/// Returns a description of the first violation.
pub fn check_script_args(latex: &str) -> Result<(), String> {
    let chars: Vec<char> = latex.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                // Skip the whole control sequence: a control word, or a
                // control symbol such as the escaped marker `\_`.
                i += 1;
                if chars.get(i).is_some_and(char::is_ascii_alphabetic) {
                    while chars.get(i).is_some_and(char::is_ascii_alphabetic) {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            },
            '^' | '_' => {
                let at = i;
                let violation = |why: &str| {
                    let context: String = chars[at..chars.len().min(at + 16)].iter().collect();
                    Err(format!("{why} at {at:?} (`{context}`) in `{latex}`"))
                };
                i += 1;
                while chars.get(i).is_some_and(|ch| *ch == ' ') {
                    i += 1;
                }
                match chars.get(i) {
                    None => return violation("marker without an argument"),
                    Some('{') => {},
                    Some('\\') => {
                        let mut j = i + 1;
                        if chars.get(j).is_some_and(char::is_ascii_alphabetic) {
                            while chars.get(j).is_some_and(char::is_ascii_alphabetic) {
                                j += 1;
                            }
                        } else {
                            j += 1;
                        }
                        if chars.get(j) == Some(&'{') {
                            return violation("control sequence with a group argument");
                        }
                        if chars[i + 1..j].iter().collect::<String>() == "char" {
                            return violation("`\\char` with its character code");
                        }
                    },
                    Some(&ch) if ch.is_whitespace() || matches!(ch, '}' | '^' | '_') => {
                        return violation("marker followed by a non-argument");
                    },
                    Some(&ch) => {
                        if ch.is_ascii_digit()
                            && chars
                                .get(i + 1)
                                .is_some_and(|&next| next.is_ascii_digit() || next == '.')
                        {
                            return violation("bare digit continued by a number");
                        }
                    },
                }
            },
            _ => i += 1,
        }
    }
    Ok(())
}
