//! Text-mode box arguments, including inline math delimited by `$` or `\(`.
use super::*;
use crate::latex::{split_token, text_style};

type Parsed<'a, T> = std::result::Result<T, nom::Err<nom::error::Error<&'a str>>>;

fn invalid(input: &str) -> nom::Err<nom::error::Error<&str>> {
    nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
}

/// TeX consumes source whitespace following a control word.  Control symbols
/// (`\ `, `\,`, escaped punctuation, and so on) do not have this lexical
/// behavior.  Keep this local to text-box scanning so an explicitly escaped or
/// group-separated space remains visible to the text decoder.
fn skip_control_word_space<'a>(token: &str, input: &'a str) -> &'a str {
    let bytes = token.as_bytes();
    let is_control_word = bytes.len() > 2
        && bytes.first() == Some(&b'\\')
        && (bytes[1].is_ascii_alphabetic() || bytes[1] == b'@');
    if is_control_word {
        input.trim_start_matches(|ch: char| ch.is_ascii_whitespace())
    } else {
        input
    }
}

/// Whether a text accent control word (`\v`, `\c`) is followed by a base its
/// text decoder can apply to within the same run, rather than ending the box
/// or meeting inline math.
fn accent_has_argument(after: &str) -> bool {
    let next = after.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
    !next.is_empty() && !next.starts_with(['}', '$']) && !next.starts_with(r"\(")
}

/// An escaped dollar is a control symbol. Dollars in nested math groups
/// (including a nested text argument) don't end the surrounding math run.
fn math_end<'a>(input: &'a str, close: &str) -> Parsed<'a, (&'a str, &'a str)> {
    let mut rest = input;
    let mut depth = 0usize;
    while !rest.is_empty() {
        if let Some(end) = crate::latex::preprocess::find_verb_end(rest, 0) {
            rest = &rest[end..];
            continue;
        }
        let (after, token) = split_token(rest).ok_or_else(|| invalid(rest))?;
        if token == close && depth == 0 {
            return Ok((after, &input[..input.len() - rest.len()]));
        }
        match token {
            "{" => depth += 1,
            "}" => depth = depth.checked_sub(1).ok_or_else(|| invalid(rest))?,
            _ => {},
        }
        rest = after;
    }
    Err(invalid(input))
}

/// Read a required text argument without expanding or interpreting its contents.
pub(super) fn argument(input: &str) -> IResult<&str, &str> {
    let (input, _) = ws(input)?;
    if input.starts_with('{') {
        crate::latex::capture_braced(input)
    } else {
        split_token(input).filter(|(_, token)| !token.starts_with(['}', '&', '#', '^', '_']))
    }
    .ok_or_else(|| invalid(input))
}

fn nested_text_style(outer: TextStyle, name: &str, requested: TextStyle) -> TextStyle {
    use TextStyle::*;
    if matches!(name, "text" | "mbox" | "hbox") {
        return outer;
    }
    if requested == Monospace || name == "textnormal" {
        return requested;
    }
    let bold = name != "textmd"
        && (matches!(
            outer,
            Bold | BoldItalic | SansSerifBold | SansSerifBoldItalic
        ) || requested == Bold);
    let was_italic = matches!(
        outer,
        Italic | BoldItalic | SansSerifItalic | SansSerifBoldItalic
    );
    let italic = match name {
        "emph" => !was_italic,
        "textup" => false,
        _ => was_italic || requested == Italic,
    };
    let sans = name != "textrm"
        && (matches!(
            outer,
            SansSerif | SansSerifBold | SansSerifItalic | SansSerifBoldItalic
        ) || requested == SansSerif);
    match (sans, bold, italic) {
        (false, false, false) => Normal,
        (false, true, false) => Bold,
        (false, false, true) => Italic,
        (false, true, true) => BoldItalic,
        (true, false, false) => SansSerif,
        (true, true, false) => SansSerifBold,
        (true, false, true) => SansSerifItalic,
        (true, true, true) => SansSerifBoldItalic,
    }
}

impl LatexCompactParser {
    pub(super) fn parse_hbox_argument<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, raw) = argument(input)?;
        let mut nodes = SmallVec::new();
        self.hbox_runs(raw, TextStyle::Normal, true, &mut nodes)?;
        let content = self.flatten_single(&nodes);
        self.ensure_node_budget(rest)?;
        Ok((rest, content))
    }

    pub(super) fn hbox_runs<'a>(
        &mut self,
        raw: &'a str,
        style: TextStyle,
        box_syntax: bool,
        nodes: &mut SmallVec<[NodeId; 8]>,
    ) -> Parsed<'a, ()> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(nom::Err::Failure(nom::error::Error::new(
                raw,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        self.depth += 1;
        let result = self.hbox_runs_inner(raw, style, box_syntax, nodes);
        self.depth -= 1;
        result
    }

    fn hbox_runs_inner<'a>(
        &mut self,
        raw: &'a str,
        style: TextStyle,
        box_syntax: bool,
        nodes: &mut SmallVec<[NodeId; 8]>,
    ) -> Parsed<'a, ()> {
        let mut rest = raw;
        let mut literal = raw;
        while !rest.is_empty() {
            let (after, token) = split_token(rest).ok_or_else(|| invalid(rest))?;
            if token == r"\char"
                && let Some((remaining, _)) = super::text_character_code(after)
            {
                // The entire escape is one literal character in the current
                // face. Keep it with this text run, even if its code names a
                // delimiter which otherwise switches between text and math.
                rest = remaining;
                continue;
            }
            let close = match token {
                "$" => Some("$"),
                r"\(" => Some(r"\)"),
                _ => None,
            };
            let name = token.strip_prefix('\\');
            let nested_style =
                name.and_then(|name| text_style(name).map(|s| nested_text_style(style, name, s)));
            let nested_box = matches!(
                token,
                r"\reflectbox" | r"\verb" | r"\LaTeX" | r"\TeX" | r"\KaTeX" | r"\textellipsis"
            );
            if close.is_none() && nested_style.is_none() && !nested_box {
                if box_syntax
                    && (matches!(token, "^" | "_" | "&" | "#" | r"\)")
                        || name.is_some_and(|name| {
                            name != "relax"
                                && !name.is_empty()
                                && name.bytes().all(|b| b.is_ascii_alphabetic())
                                && (super::text_accent::accent_mark(name).is_none()
                                    || !accent_has_argument(after))
                                && super::text_accent::special_letter(name).is_none()
                                && crate::unicode::symbols::latex_to_unicode(name).is_none()
                                && crate::latex::packages::lookup_package_symbol(name).is_none()
                        }))
                {
                    return Err(invalid(rest));
                }
                rest = skip_control_word_space(token, after);
                continue;
            }
            self.push_hbox_text(&literal[..literal.len() - rest.len()], style, nodes);
            self.ensure_node_budget(rest)?;
            if let Some(close) = close {
                let (remaining, math) = math_end(after, close)?;
                let pending_script = self.implicit_base_script.take();
                let parsed = self.parse_node_list(math);
                self.implicit_base_script = pending_script;
                let (left, math_nodes) = parsed?;
                if !left.is_empty() {
                    return Err(invalid(left));
                }
                let math_nodes = self.resolve_infix_fraction(math_nodes);
                nodes.push(self.flatten_single(&math_nodes));
                rest = remaining;
            } else if let Some(inner_style) = nested_style {
                let (remaining, inner) = argument(after)?;
                self.hbox_runs(inner, inner_style, box_syntax, nodes)?;
                rest = remaining;
            } else {
                let (remaining, inner) = self.parse_command(rest)?;
                self.push_hbox_node(inner, nodes);
                // Space after an argument is not space after a control word.
                rest = if matches!(token, r"\reflectbox" | r"\verb") {
                    remaining
                } else {
                    skip_control_word_space(token, remaining)
                };
            }
            self.ensure_node_budget(rest)?;
            literal = rest;
        }
        self.push_hbox_text(literal, style, nodes);
        self.ensure_node_budget(rest)
    }

    fn push_hbox_text(&mut self, raw: &str, style: TextStyle, nodes: &mut SmallVec<[NodeId; 8]>) {
        if !raw.is_empty() {
            let text = clean_text_content(raw);
            if !text.is_empty() {
                let sid = self.arena.push_str(&text);
                let node = self.arena.push_node(CompactNode::Text(style, sid));
                self.push_hbox_node(node, nodes);
            }
        }
    }

    fn push_hbox_node(&mut self, node: NodeId, nodes: &mut SmallVec<[NodeId; 8]>) {
        if let CompactNode::Text(style, sid) = *self.arena.node(node)
            && let Some(&last) = nodes.last()
            && let CompactNode::Text(last_style, last_sid) = *self.arena.node(last)
            && last_style == style
        {
            // A logo/control word such as `\KaTeX` is still text in the
            // surrounding hbox. Merge adjacent runs of the same style so a
            // source text group stays one canonical text node instead of
            // gaining a structural brace row.
            let mut joined = String::with_capacity(
                self.arena.str_val(last_sid).len() + self.arena.str_val(sid).len(),
            );
            joined.push_str(self.arena.str_val(last_sid));
            joined.push_str(self.arena.str_val(sid));
            let joined_sid = self.arena.push_str(&joined);
            nodes.pop();
            nodes.push(self.arena.push_node(CompactNode::Text(style, joined_sid)));
        } else {
            nodes.push(node);
        }
    }
}
