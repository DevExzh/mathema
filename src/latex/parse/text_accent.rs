//! Text-mode accents (`\'e`, `\v{c}`, `\"{\i}`) and special letters (`\ss`,
//! `\o`, `\AE`) as supported by KaTeX in `\text{…}`.
//!
//! An accent applied to a base letter yields the precomposed character when
//! one exists (Latin-1, Latin Extended-A/B, and a few common Latin Extended
//! Additional letters), and otherwise the base followed by the combining
//! diacritic.

/// Combining diacritic for a text-mode accent command name (control symbol
/// such as `'` or control word such as `v`).
pub(super) fn accent_mark(name: &str) -> Option<char> {
    Some(match name {
        "'" => '\u{0301}',
        "`" => '\u{0300}',
        "^" => '\u{0302}',
        "~" => '\u{0303}',
        "\"" => '\u{0308}',
        "=" => '\u{0304}',
        "." => '\u{0307}',
        "u" => '\u{0306}',
        "r" => '\u{030A}',
        "H" => '\u{030B}',
        "v" => '\u{030C}',
        "c" => '\u{0327}',
        "k" => '\u{0328}',
        "d" => '\u{0323}',
        "b" => '\u{0331}',
        _ => return None,
    })
}

/// Text-mode special letter commands.
pub(super) fn special_letter(name: &str) -> Option<char> {
    Some(match name {
        "i" => '\u{0131}',
        "j" => '\u{0237}',
        "ss" => '\u{00DF}',
        "o" => '\u{00F8}',
        "O" => '\u{00D8}',
        "l" => '\u{0142}',
        "L" => '\u{0141}',
        "oe" => '\u{0153}',
        "OE" => '\u{0152}',
        "ae" => '\u{00E6}',
        "AE" => '\u{00C6}',
        "aa" => '\u{00E5}',
        "AA" => '\u{00C5}',
        _ => return None,
    })
}

/// `(combining mark, bases, precomposed)`: the n-th char of `bases` combined
/// with the mark is the n-th char of `precomposed`.
const COMPOSE: &[(char, &str, &str)] = &[
    (
        '\u{0301}',
        "aeiouyAEIOUYcCnNsSzZlLrRgGæÆøØ",
        "áéíóúýÁÉÍÓÚÝćĆńŃśŚźŹĺĹŕŔǵǴǽǼǿǾ",
    ),
    ('\u{0300}', "aeiounAEIOUN", "àèìòùǹÀÈÌÒÙǸ"),
    (
        '\u{0302}',
        "aeiouAEIOUcCgGhHjJsSwWyY",
        "âêîôûÂÊÎÔÛĉĈĝĜĥĤĵĴŝŜŵŴŷŶ",
    ),
    ('\u{0303}', "aonAONiIuUeEyY", "ãõñÃÕÑĩĨũŨẽẼỹỸ"),
    ('\u{0308}', "aeiouyAEIOUY", "äëïöüÿÄËÏÖÜŸ"),
    ('\u{0304}', "aeiouyAEIOUY", "āēīōūȳĀĒĪŌŪȲ"),
    ('\u{0306}', "aegiouAEGIOU", "ăĕğĭŏŭĂĔĞĬŎŬ"),
    ('\u{0307}', "cegzCEGIZ", "ċėġżĊĖĠİŻ"),
    ('\u{030A}', "auAU", "åůÅŮ"),
    ('\u{030B}', "ouOU", "őűŐŰ"),
    (
        '\u{030C}',
        "cdenrstzlaiougkjhCDENRSTZLAIOUGKH",
        "čďěňřšťžľǎǐǒǔǧǩǰȟČĎĚŇŘŠŤŽĽǍǏǑǓǦǨȞ",
    ),
    ('\u{0327}', "cCsStTgGkKlLnNrR", "çÇşŞţŢģĢķĶļĻņŅŗŖ"),
    ('\u{0328}', "aeiouAEIOU", "ąęįǫųĄĘĮǪŲ"),
];

fn compose(base: char, mark: char) -> Option<char> {
    let (_, bases, composed) = COMPOSE.iter().find(|(m, _, _)| *m == mark)?;
    let index = bases.chars().position(|c| c == base)?;
    composed.chars().nth(index)
}

/// Append `base_text` with `mark` applied to its first character. Dotless
/// `\i`/`\j` take an accent above as the ordinary dotted letter's precomposed
/// form (`\'{\i}` → í), as the dot is replaced by the accent.
pub(super) fn push_accented(out: &mut String, base_text: &str, mark: char) {
    let mut chars = base_text.chars();
    let Some(base) = chars.next() else {
        return;
    };
    let above = !matches!(mark, '\u{0327}' | '\u{0328}' | '\u{0323}' | '\u{0331}');
    let dotted = match base {
        '\u{0131}' if above => Some('i'),
        '\u{0237}' if above => Some('j'),
        _ => None,
    };
    if let Some(ch) = compose(base, mark).or_else(|| dotted.and_then(|d| compose(d, mark))) {
        out.push(ch);
    } else {
        out.push(base);
        out.push(mark);
    }
    out.push_str(chars.as_str());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_table_is_aligned() {
        for (mark, bases, composed) in COMPOSE {
            assert_eq!(
                bases.chars().count(),
                composed.chars().count(),
                "{mark:?}: {bases}"
            );
        }
    }
}
