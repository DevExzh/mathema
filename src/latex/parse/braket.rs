//! Scoped separator handling for the braket package.
use super::*;
use crate::latex::{split_token, text_style};

impl LatexCompactParser {
    /// Build the fixed `\mathinner{\langle{#1}\rangle}` form directly. The
    /// argument remains one child of the inner row, matching the explicit
    /// group in the package expansion while avoiding a full preprocessor pass.
    pub(super) fn parse_fixed_braket<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, content) = self.parse_required_group(input)?;
        let open_sid = self.arena.push_str("\u{27E8}");
        let close_sid = self.arena.push_str("\u{27E9}");
        let open = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Open, open_sid));
        let close = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Close, close_sid));
        let children = self.arena.push_children_deferred(&[open, content, close]);
        let row = self.arena.push_node(CompactNode::Row(children));
        Ok((
            input,
            self.arena.push_node(CompactNode::MathAtom {
                atom_type: MathAtomType::Inner,
                content: row,
            }),
        ))
    }

    pub(super) fn parse_braket_macro<'a>(
        &mut self,
        input: &'a str,
        syntax: &'static Syntax,
    ) -> IResult<&'a str, NodeId> {
        let (rest, body) = hbox::argument(input)?;
        let expanded = expand(
            body,
            syntax,
            MAX_PARSE_DEPTH.saturating_sub(self.depth) as usize,
        )?;
        self.open_buffer(input, &expanded);
        let parsed = self.parse_math_list(&expanded);
        self.close_buffer();
        let (leftover, ids) = parsed.map_err(|err| err.map_input(|_| input))?;
        if !leftover.trim().is_empty() {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
        Ok((rest, self.flatten_single(&ids)))
    }
}

pub(super) struct Syntax {
    open: &'static str,
    close: &'static str,
    single: &'static str,
    double: Option<&'static str>,
    first_only: bool,
}

pub(super) const BRAKET: &Syntax = &Syntax {
    open: r"\left\langle ",
    close: r"\right\rangle ",
    single: r"\,\middle\vert\,",
    double: Some(r"\,\middle\Vert\,"),
    first_only: false,
};
pub(super) const SET: &Syntax = &Syntax {
    open: r"\left\{\:",
    close: r"\:\right\}",
    single: r"\;\middle\vert\;",
    double: Some(r"\;\middle\Vert\;"),
    first_only: true,
};
pub(super) const FIXED_SET: &Syntax = &Syntax {
    open: r"\{\,",
    close: r"\,\}",
    single: r"\mid ",
    double: None,
    first_only: true,
};

impl Syntax {
    fn command(token: &str) -> Option<&'static Self> {
        match token {
            r"\Braket" => Some(BRAKET),
            r"\Set" => Some(SET),
            r"\set" => Some(FIXED_SET),
            _ => None,
        }
    }

    fn replace_bar(&self, token: &str, rest: &mut &str, out: &mut String) -> bool {
        let replacement = match (token, self.double) {
            ("|", Some(double)) if rest.starts_with('|') => {
                *rest = &rest[1..];
                double
            },
            ("|", _) => self.single,
            (r"\|", Some(double)) => double,
            _ => return false,
        };
        out.push_str(replacement);
        true
    }
}

/// Expand each nested separator scope into the same buffer. Its closing brace
/// restores the outer separator rules, including whether its first bar was used.
fn expand<'a>(
    mut input: &'a str,
    mut syntax: &'static Syntax,
    depth_limit: usize,
) -> std::result::Result<String, nom::Err<nom::error::Error<&'a str>>> {
    let invalid = |s| nom::Err::Error(nom::error::Error::new(s, nom::error::ErrorKind::Char));
    let mut out = String::with_capacity(input.len() + 32);
    let mut scopes = SmallVec::<[(usize, &Syntax, bool); 4]>::new();
    let mut depth = 0usize;
    let mut active = true;
    out.push_str(syntax.open);
    while !input.is_empty() {
        if let Some(end) = crate::latex::preprocess::find_verb_end(input, 0) {
            out.push_str(&input[..end]);
            input = &input[end..];
            continue;
        }
        let (rest, token) = split_token(input).ok_or_else(|| invalid(input))?;
        input = rest;
        if active && syntax.replace_bar(token, &mut input, &mut out) {
            active = !syntax.first_only;
            continue;
        }
        if let Some(nested) = Syntax::command(token) {
            let (arg, _) = ws(input)?;
            out.push_str(nested.open);
            if let Some(body) = arg.strip_prefix('{') {
                if scopes.len() >= depth_limit {
                    return Err(nom::Err::Failure(nom::error::Error::new(
                        arg,
                        nom::error::ErrorKind::TooLarge,
                    )));
                }
                scopes.push((depth, syntax, active));
                depth += 1;
                syntax = nested;
                active = true;
                input = body;
            } else {
                let (tail, atom) = hbox::argument(arg)?;
                if !nested.replace_bar(atom, &mut "", &mut out) {
                    out.push_str(atom);
                }
                out.push_str(nested.close);
                input = tail;
            }
            continue;
        }
        if token == "{" {
            depth += 1;
        } else if token == "}" {
            depth = depth.checked_sub(1).ok_or_else(|| invalid(input))?;
            if scopes
                .last()
                .is_some_and(|(outer_depth, _, _)| *outer_depth == depth)
            {
                out.push_str(syntax.close);
                let (_, outer, enabled) = scopes.pop().unwrap();
                syntax = outer;
                active = enabled;
                continue;
            }
        }
        out.push_str(token);
        // Text arguments and explicit delimiter operands keep their literal bars.
        let after = match token {
            r"\left" | r"\right" | r"\middle" => split_token(ws(input)?.0).map(|(tail, _)| tail),
            r"\char" => ws(input)?
                .0
                .strip_prefix('`')
                .and_then(split_token)
                .map(|(tail, _)| tail),
            _ if token.strip_prefix('\\').is_some_and(|name| {
                text_style(name).is_some() || matches!(name, "textsl" | "textsc")
            }) =>
            {
                hbox::argument(input).ok().map(|(tail, _)| tail)
            },
            _ => None,
        };
        if let Some(tail) = after {
            out.push_str(&input[..input.len() - tail.len()]);
            input = tail;
        }
    }
    if depth != 0 {
        return Err(invalid(input));
    }
    out.push_str(syntax.close);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_arguments_and_explicit_delimiters_keep_their_bars() {
        for body in [r"\text{a|b}", r"\verb!a|b!", r"\left|x\right|", r"\char`|"] {
            let source = format!(r"\Set{{{body}|P}}");
            let explicit = format!(r"\left\{{\: {body}\;\middle\vert\; P\:\right\}}");
            let xml = |s: &str| crate::from_latex(s).unwrap().to_mathml().unwrap();
            assert_eq!(xml(&source), xml(&explicit), "{source}");
        }
    }

    #[test]
    fn nested_sets_charge_the_available_scope_depth() {
        let source = r"\set{\set{x|y}|z}";
        assert!(expand(source, SET, 1).is_err());
        assert!(expand(source, SET, 2).is_ok());
    }
}
