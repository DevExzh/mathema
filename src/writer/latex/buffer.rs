//! The LaTeX writer's output.
//!
//! Text is only added through [`LatexBuffer`], which ends a control word
//! before a letter that would extend its name: whatever was or was not written
//! in between, `\int` and an identifier `eractionmode` stay `\int eractionmode`.

use super::LossReport;
use crate::ast::Rational;
use crate::unicode::tex_letters::is_tex_letter;

/// A named operator gives literal punctuation different TeX math codes.
pub(super) struct LatexBuffer {
    text: String,
    /// Where the output ended when it last ended with a control word.
    word_end: usize,
    pub(super) in_operator_name: bool,
    /// Whether undefined commands are written live
    /// ([`UndefinedCommands::Keep`](crate::UndefinedCommands::Keep)).
    pub(super) keep_undefined: bool,
    /// Where the output of each undefined command written as a live control
    /// word ends. What the writer puts next is only known at the end: a group
    /// there would read back as another argument of the command.
    command_ends: Vec<usize>,
    /// What the output does not keep.
    pub(super) report: LossReport,
    /// Every control word the writer wrote.
    #[cfg(debug_assertions)]
    words: std::collections::HashSet<String>,
}

/// Whether `ch` after a control word is part of its name: in TeX a letter, in
/// KaTeX also `@`, in XeTeX and LuaTeX also a letter outside ASCII.
#[inline]
fn extends_word(ch: char) -> bool {
    match ch {
        'a'..='z' | 'A'..='Z' | '@' => true,
        _ => !ch.is_ascii() && is_tex_letter(ch),
    }
}

/// Whether `text` ends with a control word.
#[inline]
fn ends_with_word(text: &str) -> bool {
    let is_letter = |byte: &u8| byte.is_ascii_alphabetic() || *byte == b'@';
    text.as_bytes().last().is_some_and(is_letter)
        && text
            .bytes()
            .rposition(|byte| !is_letter(&byte))
            .is_some_and(|at| text.as_bytes()[at] == b'\\' && is_escape(&text[..=at]))
}

/// Whether the backslash `text` ends with starts a control sequence.
fn is_escape(text: &str) -> bool {
    !(text.len() - text.trim_end_matches('\\').len()).is_multiple_of(2)
}

/// The names of the control words in `text`, as the widest lexer reads them.
#[cfg(debug_assertions)]
fn control_words(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        loop {
            rest = &rest[rest.find('\\')? + 1..];
            let end = rest.find(|ch| !extends_word(ch)).unwrap_or(rest.len());
            let (word, after) = rest.split_at(end);
            if !word.is_empty() {
                rest = after;
                return Some(word);
            }
            let mut chars = rest.chars();
            chars.next();
            rest = chars.as_str();
        }
    })
}

/// Some of `text` around the control word `word`.
#[cfg(debug_assertions)]
fn excerpt<'a>(text: &'a str, word: &str) -> &'a str {
    const AROUND: usize = 60;
    let at = text.find(&format!("\\{word}")).unwrap_or(0);
    let boundary = |mut at: usize| {
        while !text.is_char_boundary(at) {
            at -= 1;
        }
        at
    };
    let end = (at + word.len() + AROUND).min(text.len());
    &text[boundary(at.saturating_sub(AROUND))..boundary(end)]
}

impl LatexBuffer {
    pub(super) fn new(keep_undefined: bool, report: LossReport) -> Self {
        Self {
            text: String::with_capacity(256),
            word_end: usize::MAX,
            in_operator_name: false,
            keep_undefined,
            command_ends: Vec::new(),
            report,
            #[cfg(debug_assertions)]
            words: std::collections::HashSet::new(),
        }
    }

    /// Write a character that is not the start of a control sequence.
    #[inline]
    pub(super) fn push(&mut self, ch: char) {
        if self.word_end == self.text.len() && extends_word(ch) {
            self.text.push(' ');
        }
        self.text.push(ch);
    }

    /// Write `text`, which holds the control words in it whole.
    #[inline]
    pub(super) fn push_str(&mut self, text: &str) {
        if self.word_end == self.text.len() && text.starts_with(extends_word) {
            self.text.push(' ');
        }
        self.record(text);
        self.text.push_str(text);
        if ends_with_word(text) {
            self.word_end = self.text.len();
        }
    }

    /// Write `width` times `multiplier` as a decimal number.
    pub(super) fn push_decimal(&mut self, width: Rational, multiplier: u32) {
        width.write_scaled_decimal(&mut self.text, multiplier);
    }

    /// Write the control sequence `\name`.
    #[inline]
    pub(super) fn command(&mut self, name: &str) {
        self.command_with_suffix(name, "");
    }

    /// Write the control sequence `\name` + `suffix`.
    pub(super) fn command_with_suffix(&mut self, name: &str, suffix: &str) {
        #[cfg(debug_assertions)]
        let start = self.text.len();
        self.text.push('\\');
        self.text.push_str(name);
        self.text.push_str(suffix);
        #[cfg(debug_assertions)]
        {
            let command = self.text[start..].to_owned();
            self.record(&command);
        }
        // A name is a word or one character.
        if self.text.ends_with(extends_word) {
            self.word_end = self.text.len();
        }
    }

    #[cfg(debug_assertions)]
    fn record(&mut self, text: &str) {
        if text.contains('\\') {
            self.words.extend(control_words(text).map(str::to_owned));
        }
    }

    #[cfg(not(debug_assertions))]
    fn record(&mut self, _text: &str) {}

    /// Note the end of an undefined command written live.
    pub(super) fn end_undefined_command(&mut self) {
        self.command_ends.push(self.text.len());
    }

    /// Insert `text`, which starts with no letter and does not end with a
    /// control word, at byte offset `at`, keeping the recorded ends on the
    /// same output.
    pub(super) fn insert_text(&mut self, at: usize, text: &str) {
        debug_assert!(!text.starts_with(extends_word) && !ends_with_word(text));
        self.record(text);
        self.text.insert_str(at, text);
        for end in &mut self.command_ends {
            if *end > at {
                *end += text.len();
            }
        }
        if self.word_end != usize::MAX && self.word_end > at {
            self.word_end += text.len();
        }
    }

    /// [`Self::insert_text`] at each of the ascending offsets `at`, in one pass.
    pub(super) fn insert_text_at_each(&mut self, at: &[usize], text: &str) {
        let Some(&first) = at.first() else {
            return;
        };
        debug_assert!(!text.starts_with(extends_word) && !ends_with_word(text));
        self.record(text);
        let tail = self.text.split_off(first);
        let mut from = first;
        for &at in at {
            self.text.push_str(&tail[from - first..at - first]);
            self.text.push_str(text);
            from = at;
        }
        self.text.push_str(&tail[from - first..]);
        let moved = |end: usize| text.len() * at.partition_point(|&at| at < end);
        for end in &mut self.command_ends {
            *end += moved(*end);
        }
        if self.word_end != usize::MAX {
            self.word_end += moved(self.word_end);
        }
    }

    /// Replace the character at byte offset `at` by `text`, which is at least
    /// as long.
    pub(super) fn replace_char(&mut self, at: usize, text: &str) {
        let len = self.text[at..].chars().next().map_or(0, char::len_utf8);
        self.record(text);
        self.text.replace_range(at..at + len, &text[..len]);
        for end in self
            .command_ends
            .iter_mut()
            .chain(Some(&mut self.word_end).filter(|end| **end != usize::MAX))
        {
            if *end > at + len {
                *end += text.len() - len;
            }
        }
        self.text.insert_str(at + len, &text[len..]);
    }

    /// Cut the output back to `len` bytes.
    pub(super) fn truncate_text(&mut self, len: usize) {
        self.text.truncate(len);
        self.command_ends.retain(|&end| end <= len);
        self.word_end = if ends_with_word(&self.text) {
            len
        } else {
            usize::MAX
        };
    }

    /// The finished output. After each undefined command, put `\relax`
    /// before a group that would read back as one more of its arguments.
    pub(super) fn finish(mut self) -> String {
        self.command_ends.sort_unstable();
        self.command_ends.dedup();
        for at in std::mem::take(&mut self.command_ends).into_iter().rev() {
            if self.text[at..].trim_start().starts_with('{') {
                self.record("\\relax");
                self.text.insert_str(at, "\\relax");
            }
        }
        #[cfg(debug_assertions)]
        for word in control_words(&self.text) {
            assert!(
                self.words.contains(word),
                "\\{word} in the output is no command the writer wrote: {}",
                excerpt(&self.text, word)
            );
        }
        self.text
    }
}

impl std::ops::Deref for LatexBuffer {
    type Target = String;
    fn deref(&self) -> &String {
        &self.text
    }
}

impl std::fmt::Write for LatexBuffer {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.push_str(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer() -> LatexBuffer {
        LatexBuffer::new(false, LossReport::default())
    }

    #[test]
    fn a_letter_never_extends_a_control_word() {
        let mut buf = buffer();
        buf.command("int");
        buf.push_str("");
        buf.push('e');
        buf.push('f');
        buf.push_str(r"\limits");
        buf.push_str("x1");
        buf.push_str("y");
        buf.command("&");
        buf.push('z');
        buf.push_str(r"\\");
        buf.push_str("a");
        buf.push_str(r"\\\a");
        buf.push('@');
        buf.command_with_suffix("big", "l");
        buf.push('(');
        buf.push('b');
        buf.push_str(r"\char`");
        buf.push('c');
        buf.push_str(r"\\b");
        buf.push('d');
        assert_eq!(
            buf.finish(),
            r"\int ef\limits x1y\&z\\a\\\a @\bigl(b\char`c\\bd"
        );
    }

    #[test]
    fn a_letter_outside_ascii_is_separated_and_other_characters_are_not() {
        let written = |text: &str| {
            let mut whole = buffer();
            whole.command("pi");
            whole.push_str(text);
            let mut apart = buffer();
            apart.command("pi");
            text.chars().for_each(|ch| apart.push(ch));
            assert_eq!(&whole[..], &apart[..]);
            whole.finish()
        };
        for text in ["é", "Χx", "ж", "中", "ℝ", "𝐀", "\u{301}"] {
            assert_eq!(written(text), format!(r"\pi {text}"));
        }
        for text in ["∂é", "→", "²", "ⅰ", "①", "。", "\u{A0}", "𝟎"] {
            assert_eq!(written(text), format!(r"\pi{text}"));
        }
    }

    /// Text is added and taken away before the end of the output.
    #[test]
    fn the_end_of_a_control_word_is_followed_through_edits() {
        let mut buf = buffer();
        buf.push('x');
        buf.command("sum");
        buf.insert_text(0, "{");
        buf.insert_text_at_each(&[1, 2], "{}");
        buf.push('y');
        assert_eq!(&buf[..], r"{{}x{}\sum y");
        buf.truncate_text(buf.len() - 2);
        buf.push('z');
        assert_eq!(&buf[..], r"{{}x{}\sum z");
        buf.truncate_text(buf.len() - 3);
        assert!(buf.ends_with(r"\su"));
        buf.push_str("m");
        assert!(buf.ends_with(r"\su m"));

        let mut buf = buffer();
        buf.push('\u{301}');
        buf.command("sum");
        buf.replace_char(0, &crate::writer::latex::escape::char_code('\u{301}', " "));
        buf.push('y');
        assert_eq!(&buf[..], r"\char769 \sum y");
    }

    /// A buffer with the undefined command `\foo` after `before`.
    fn undefined(before: &str) -> LatexBuffer {
        let mut buf = LatexBuffer::new(true, LossReport::default());
        buf.push_str(before);
        buf.command("foo");
        buf.end_undefined_command();
        buf
    }

    /// The end of an undefined command is followed through edits, too.
    #[test]
    fn a_group_after_an_undefined_command_is_set_apart_after_edits() {
        // (`insert_str` before the command moves its end.)
        let mut buf = undefined("x");
        buf.insert_text(0, "{-}");
        buf.push_str("{a}");
        assert_eq!(buf.finish(), r"{-}x\foo\relax{a}");
        // After it, it does not.
        let mut buf = undefined("x");
        buf.insert_text(buf.len(), "{}");
        assert_eq!(buf.finish(), r"x\foo\relax{}");

        let mut buf = undefined("xy");
        buf.insert_text_at_each(&[0, 1, 2], "{-}");
        buf.push_str("{a}");
        assert_eq!(buf.finish(), r"{-}x{-}y{-}\foo\relax{a}");

        // A command that was cut off has no end.
        let mut buf = undefined("xyz");
        buf.truncate_text(2);
        buf.push_str("12345{a}");
        assert_eq!(buf.finish(), "xy12345{a}");
        let mut buf = undefined("xyz");
        let len = buf.len();
        buf.push_str("{a}");
        buf.truncate_text(len);
        buf.push_str("{b}");
        assert_eq!(buf.finish(), r"xyz\foo\relax{b}");

        // Space does not keep a group from being an argument.
        let mut buf = undefined("");
        buf.push_str("  {a}");
        assert_eq!(buf.finish(), r"\foo\relax  {a}");
        let mut buf = undefined("");
        buf.push_str(" a{b}");
        assert_eq!(buf.finish(), r"\foo a{b}");
    }

    #[test]
    #[cfg(debug_assertions)]
    fn an_excerpt_is_short_and_whole_characters() {
        let text = format!(r"{}\foo{}", "é".repeat(500), "é".repeat(500));
        let shown = excerpt(&text, "foo");
        assert!(shown.contains(r"\foo") && shown.len() < 200);
        assert_eq!(excerpt(r"\foo", "foo"), r"\foo");
        let panic = std::panic::catch_unwind(|| {
            let mut buf = buffer();
            buf.push_str("\\");
            buf.push_str("input");
            buf.push_str(&" é".repeat(500));
            buf.finish()
        })
        .unwrap_err();
        assert!(panic.downcast_ref::<String>().unwrap().len() < 300);
    }

    /// No dangerous command comes out of a command and letters after it.
    #[test]
    fn no_dangerous_command_is_made_of_two_parts() {
        for name in crate::ast::undefined::dangerous_names() {
            for split in 1..name.len() {
                let mut buf = buffer();
                buf.command(&name[..split]);
                buf.push_str(&name[split..]);
                let expected = format!("\\{} {}", &name[..split], &name[split..]);
                if name[..split].ends_with(extends_word) && name[split..].starts_with(extends_word)
                {
                    assert_eq!(buf.finish(), expected);
                }
            }
        }
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic = "no command the writer wrote"]
    fn a_control_word_the_writer_did_not_write_fails_debug_builds() {
        let mut buf = buffer();
        buf.push_str("\\");
        buf.push_str("input");
        buf.finish();
    }
}
