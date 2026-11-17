//! Ordered, scoped TeX macro expansion.
//!
//! The original preprocessor collected every definition before expanding the
//! input.  That is convenient, but it is not TeX's model: a definition becomes
//! visible at the point where it is read and ordinary definitions disappear at
//! the end of their group.  This module keeps the token stream and the current
//! definition stack together while walking the input.  It intentionally does
//! not know anything about the AST parser; its result is ordinary LaTeX text.
//!
//! A small but important detail is that a `\let` binding stores a *meaning*,
//! rather than an alias to a name.  Macro meanings are cloned at the `\let`
//! site, while primitive/control-sequence meanings retain their token identity
//! and are marked frozen when emitted.  This is enough to keep later
//! redefinitions from changing a `\let` copy without teaching this module all
//! of the parser's primitive commands.

use std::collections::HashMap;

use super::packages;
use super::{expansion_cap, find_verb_end};
use crate::latex::text_style;

const MAX_EXPANSION_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Char(char),
    BeginGroup,
    EndGroup,
    ControlWord(String),
    ControlSymbol(char),
    /// A verbatim region is opaque to both definition handling and expansion.
    Verbatim(String),
    /// A frozen token is emitted literally.  It is used for `\let` copies of
    /// primitive tokens and for the one-token protection supplied by
    /// `\noexpand` in ordinary expansion.
    Frozen(Box<Token>),
}

impl Token {
    fn is_whitespace(&self) -> bool {
        matches!(self, Self::Char(c) if c.is_ascii_whitespace())
    }

    fn is_control_word(&self, name: &str) -> bool {
        matches!(self, Self::ControlWord(n) if n == name)
    }

    fn byte_len(&self) -> usize {
        match self {
            Self::Char(c) => c.len_utf8(),
            Self::BeginGroup | Self::EndGroup => 1,
            Self::ControlWord(name) => 1 + name.len(),
            Self::ControlSymbol(c) => 1 + c.len_utf8(),
            Self::Verbatim(s) => s.len(),
            Self::Frozen(inner) => inner.byte_len(),
        }
    }
}

#[derive(Clone, Debug)]
struct MacroDef {
    param_count: u8,
    optional_default: Option<Vec<Token>>,
    body: Vec<Token>,
}

#[derive(Clone, Debug)]
struct EnvDef {
    begin: Vec<Token>,
    end: Vec<Token>,
}

#[derive(Clone, Debug)]
enum Meaning {
    Macro(MacroDef),
    /// A token meaning captured by `\let`.  A control token is emitted frozen
    /// so a later definition of that control sequence cannot alter the copy.
    Token(Token),
}

#[derive(Clone, Debug, Default)]
struct Scope {
    macros: HashMap<String, Meaning>,
    environments: HashMap<String, EnvDef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    /// Expansion while constructing an `\edef`/`\xdef`.  Definitions and
    /// other unexpandable primitives are copied as tokens; ordinary macros are
    /// expanded and `\noexpand` protects one following token.
    Edef,
}

/// Expand one source string according to TeX's ordered definition rules.
///
/// The caller supplies the original input length so the same cap as the
/// legacy preprocessor is retained.  The returned text is bounded while it is
/// built, not after a potentially dangerous allocation has already happened.
pub(crate) fn expand(input: &str) -> Result<String, usize> {
    let cap = expansion_cap(input.len());
    let tokens = tokenize(input);
    let mut processor = Processor::new(cap);
    processor.seed_builtins();
    let mut out = Vec::with_capacity(input.len());
    processor.process_tokens(&tokens, &mut out, Mode::Normal, 0)?;
    render_tokens(&out)
        .map_err(|_| cap)
}

struct Processor {
    scopes: Vec<Scope>,
    cap: usize,
    /// Total generated token bytes.  Counting incrementally keeps runaway
    /// expansion checks O(n); repeatedly summing the output vector makes a
    /// doubling macro accidentally quadratic before the cap is reached.
    produced: usize,
    /// Known environment nesting is only needed for environments introduced by
    /// `\newenvironment`; parser-owned environments remain ordinary tokens.
    environment_depth: usize,
}

impl Processor {
    fn new(cap: usize) -> Self {
        Self {
            scopes: vec![Scope::default()],
            cap,
            produced: 0,
            environment_depth: 0,
        }
    }

    fn seed_builtins(&mut self) {
        for (name, param_count, body) in packages::builtin_macros() {
            self.define_macro(
                name.to_owned(),
                Meaning::Macro(MacroDef {
                    param_count,
                    optional_default: None,
                    body: tokenize(body),
                }),
                true,
            );
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        // The global scope must remain alive even for malformed input with an
        // unmatched closing brace.
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn define_macro(&mut self, name: String, meaning: Meaning, global: bool) {
        let index = if global { 0 } else { self.scopes.len() - 1 };
        self.scopes[index].macros.insert(name, meaning);
    }

    fn define_environment(&mut self, name: String, env: EnvDef, global: bool) {
        let index = if global { 0 } else { self.scopes.len() - 1 };
        self.scopes[index].environments.insert(name, env);
    }

    fn lookup_macro(&self, name: &str) -> Option<Meaning> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.macros.get(name).cloned())
    }

    fn lookup_environment(&self, name: &str) -> Option<EnvDef> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.environments.get(name).cloned())
    }

    fn append(&mut self, out: &mut Vec<Token>, token: Token) -> Result<(), usize> {
        let next = self.produced.saturating_add(token.byte_len());
        if next > self.cap {
            return Err(self.cap);
        }
        self.produced = next;
        out.push(token);
        Ok(())
    }

    /// Process `input[start..]`, returning the first token not consumed.  A
    /// recursive call is used only for a matched group or macro replacement;
    /// the depth guard prevents a self-referential macro from overflowing the
    /// Rust stack while retaining the historical bounded-expansion behavior.
    fn process_tokens(
        &mut self,
        input: &[Token],
        out: &mut Vec<Token>,
        mode: Mode,
        expansion_depth: usize,
    ) -> Result<(), usize> {
        let mut i = 0;
        while i < input.len() {
            match &input[i] {
                Token::BeginGroup => {
                    self.append(out, Token::BeginGroup)?;
                    self.push_scope();
                    let before = out.len();
                    let next = self.process_group(input, i + 1, out, mode, expansion_depth)?;
                    // `process_group` leaves a malformed unterminated group
                    // at EOF in the same state the parser will report; restore
                    // the scope here so a following top-level token cannot be
                    // accidentally captured by it.
                    self.pop_scope();
                    if next == input.len() && !matches!(input.last(), Some(Token::EndGroup)) {
                        // The opening brace itself was emitted above.  Nothing
                        // special is required; this branch documents why the
                        // scope is restored on malformed input.
                        let _ = before;
                    }
                    i = next;
                },
                Token::EndGroup => {
                    // An unmatched close is deliberately preserved for the
                    // parser's normal strict/lenient error handling.
                    self.append(out, Token::EndGroup)?;
                    i += 1;
                },
                Token::Verbatim(raw) => {
                    self.append(out, Token::Verbatim(raw.clone()))?;
                    i += 1;
                },
                Token::ControlWord(name) => {
                    if mode == Mode::Edef {
                        if self.process_edef_command(input, &mut i, out, expansion_depth)? {
                            continue;
                        }
                        // An unexpandable command is retained as its original
                        // token.  This is what lets an `\edef` keep `\def`,
                        // `\let`, `\kern`, and parser primitives intact.
                        self.append(out, input[i].clone())?;
                        i += 1;
                        continue;
                    }

                    if self.process_definition_command(input, &mut i, out)? {
                        continue;
                    }
                    if self.process_environment_command(input, &mut i, out, expansion_depth)? {
                        continue;
                    }
                    if name == "noexpand" {
                        self.process_noexpand(input, &mut i, out)?;
                        continue;
                    }
                    if self.process_macro_command(input, &mut i, out, expansion_depth)? {
                        continue;
                    }
                    if self.wrap_text_macro_argument(input, &mut i, out, expansion_depth)? {
                        continue;
                    }
                    self.append(out, input[i].clone())?;
                    i += 1;
                },
                _ => {
                    self.append(out, input[i].clone())?;
                    i += 1;
                },
            }
        }
        Ok(())
    }

    /// Process a group beginning immediately after its opening brace.  The
    /// matching closing brace is emitted and consumed by this function.
    fn process_group(
        &mut self,
        input: &[Token],
        start: usize,
        out: &mut Vec<Token>,
        mode: Mode,
        expansion_depth: usize,
    ) -> Result<usize, usize> {
        let mut i = start;
        while i < input.len() {
            if matches!(input[i], Token::EndGroup) {
                self.append(out, Token::EndGroup)?;
                return Ok(i + 1);
            }
            // Processing a suffix in a group would make a nested group pop the
            // wrong scope, so use the same loop as `process_tokens` but stop at
            // this group's close.  The small helper copies the code path while
            // retaining the exact current scope.
            let begin = i;
            if matches!(input[i], Token::BeginGroup) {
                self.append(out, Token::BeginGroup)?;
                self.push_scope();
                i = self.process_group(input, i + 1, out, mode, expansion_depth)?;
                self.pop_scope();
                continue;
            }
            match &input[i] {
                Token::Verbatim(raw) => {
                    self.append(out, Token::Verbatim(raw.clone()))?;
                    i += 1;
                },
                Token::ControlWord(name) if mode == Mode::Edef => {
                    if self.process_edef_command(input, &mut i, out, expansion_depth)? {
                        continue;
                    }
                    self.append(out, input[i].clone())?;
                    i += 1;
                },
                Token::ControlWord(name) => {
                    if self.process_definition_command(input, &mut i, out)? {
                        continue;
                    }
                    if self.process_environment_command(input, &mut i, out, expansion_depth)? {
                        continue;
                    }
                    if name == "noexpand" {
                        self.process_noexpand(input, &mut i, out)?;
                        continue;
                    }
                    if self.process_macro_command(input, &mut i, out, expansion_depth)? {
                        continue;
                    }
                    if self.wrap_text_macro_argument(input, &mut i, out, expansion_depth)? {
                        continue;
                    }
                    self.append(out, input[i].clone())?;
                    i += 1;
                },
                _ => {
                    self.append(out, input[i].clone())?;
                    i += 1;
                },
            }
            debug_assert!(i > begin || input.is_empty());
        }
        Ok(i)
    }

    /// Handle a definition command at `*index`, consuming it on success.
    fn process_definition_command(
        &mut self,
        input: &[Token],
        index: &mut usize,
        _out: &mut Vec<Token>,
    ) -> Result<bool, usize> {
        let name = match &input[*index] {
            Token::ControlWord(name) => name.as_str(),
            _ => return Ok(false),
        };

        let (global, command_index) = if matches!(name, "global" | "long") {
            self.parse_definition_prefix(input, *index)?
        } else {
            (false, *index)
        };
        if command_index != *index {
            let command_name = match &input[command_index] {
                Token::ControlWord(command) => command.as_str(),
                _ => return Ok(false),
            };
            if command_name == "let" {
                if let Some(end) = self.parse_let(input, command_index, true, index)? {
                    *index = end;
                    return Ok(true);
                }
                return Ok(false);
            }
            if !matches!(command_name, "def" | "gdef" | "edef" | "xdef") {
                return Ok(false);
            }
            if let Some(end) = self.parse_def(
                input,
                command_index,
                global || command_name == "gdef" || command_name == "xdef",
                matches!(command_name, "edef" | "xdef"),
                index,
            )? {
                *index = end;
                return Ok(true);
            }
            return Ok(false);
        }

        match name {
            "def" | "gdef" | "edef" | "xdef" => {
                if let Some(end) = self.parse_def(
                    input,
                    *index,
                    name == "gdef" || name == "xdef",
                    matches!(name, "edef" | "xdef"),
                    index,
                )? {
                    *index = end;
                    Ok(true)
                } else {
                    Ok(false)
                }
            },
            "let" => {
                if let Some(end) = self.parse_let(input, *index, false, index)? {
                    *index = end;
                    Ok(true)
                } else {
                    Ok(false)
                }
            },
            "newcommand" | "renewcommand" | "providecommand" => {
                if let Some(end) = self.parse_newcommand(input, *index, name, index)? {
                    *index = end;
                    Ok(true)
                } else {
                    Ok(false)
                }
            },
            "newenvironment" => {
                if let Some(end) = self.parse_newenvironment(input, *index, index)? {
                    *index = end;
                    Ok(true)
                } else {
                    Ok(false)
                }
            },
            _ => Ok(false),
        }
    }

    /// Parse one or more `\global`/`\long` prefixes and return the command
    /// index.  A macro expanding to `\def` is accepted, matching TeX's prefix
    /// behavior without expanding arbitrary commands in a definition header.
    fn parse_definition_prefix(
        &self,
        input: &[Token],
        start: usize,
    ) -> Result<(bool, usize), usize> {
        let mut i = start;
        let mut global = false;
        loop {
            let Token::ControlWord(name) = &input[i] else {
                return Ok((global, i));
            };
            if name == "global" {
                global = true;
                i += 1;
                continue;
            }
            if name == "long" {
                i += 1;
                continue;
            }
            if matches!(name.as_str(), "def" | "gdef" | "edef" | "xdef" | "let") {
                return Ok((global, i));
            }
            if let Some(Meaning::Macro(def)) = self.lookup_macro(name)
                && def.param_count == 0
                && def.body.len() == 1
                && let Token::ControlWord(command) = &def.body[0]
                && matches!(command.as_str(), "def" | "gdef" | "edef" | "xdef")
            {
                // The parser consumes the macro token as a prefix target, but
                // parse_def needs the effective command's index.  Returning
                // this index is not enough to remove the token from the input;
                // parse_def handles that one-token indirection below.
                return Ok((global, i));
            }
            return Ok((global, i));
        }
    }

    fn parse_def(
        &mut self,
        input: &[Token],
        start: usize,
        global: bool,
        expand_body: bool,
        index: &mut usize,
    ) -> Result<Option<usize>, usize> {
        let mut i = start;
        let mut command = match &input[i] {
            Token::ControlWord(name) => name.clone(),
            _ => return Ok(None),
        };
        if !matches!(command.as_str(), "def" | "gdef" | "edef" | "xdef") {
            // `\global\DEF` where DEF expands to \def.
            let Some(Meaning::Macro(def)) = self.lookup_macro(&command) else {
                return Ok(None);
            };
            if def.param_count != 0 || def.body.len() != 1 {
                return Ok(None);
            }
            let Token::ControlWord(effective) = &def.body[0] else {
                return Ok(None);
            };
            if !matches!(effective.as_str(), "def" | "gdef" | "edef" | "xdef") {
                return Ok(None);
            }
            command = effective.clone();
            i += 1;
        }
        i += 1;
        i = skip_ws(input, i);
        let Some((after_name, name)) = control_word_at(input, i) else {
            return Ok(None);
        };
        i = after_name;

        let mut param_count = 0u8;
        loop {
            let next = skip_ws(input, i);
            if next + 1 < input.len()
                && matches!(input[next], Token::Char('#'))
                && let Token::Char(digit) = input[next + 1]
                && digit.is_ascii_digit()
            {
                let n = digit.to_digit(10).unwrap_or(0);
                if n == 0 || n != u32::from(param_count) + 1 || n > 9 {
                    // Delimited parameter text and malformed parameter numbers
                    // are deliberately left for the dedicated macro-argument
                    // module / parser rather than silently accepting them.
                    return Ok(None);
                }
                param_count += 1;
                i = next + 2;
                continue;
            }
            i = next;
            break;
        }
        let Some((after_body, body)) = group_at(input, i) else {
            return Ok(None);
        };
        let body = if expand_body {
            let mut expanded = Vec::with_capacity(body.len());
            self.process_tokens(&body, &mut expanded, Mode::Edef, 0)?;
            expanded
        } else {
            body
        };
        self.define_macro(
            name,
            Meaning::Macro(MacroDef {
                param_count,
                optional_default: None,
                body,
            }),
            global,
        );
        *index = after_body;
        let _ = command;
        Ok(Some(after_body))
    }

    fn parse_let(
        &mut self,
        input: &[Token],
        start: usize,
        global: bool,
        _index: &mut usize,
    ) -> Result<Option<usize>, usize> {
        let mut i = skip_ws(input, start + 1);
        let Some((after_target, target)) = control_word_at(input, i) else {
            return Ok(None);
        };
        i = skip_ws(input, after_target);
        if matches!(input.get(i), Some(Token::Char('='))) {
            i += 1;
            // TeX permits one optional space after the equals sign.  Preserve
            // a lone space immediately before a group close as the source
            // token itself (`\let\space= `), while skipping separator spaces
            // before an ordinary source token.
            if matches!(input.get(i), Some(Token::Char(' ')))
                && !matches!(input.get(i + 1), Some(Token::EndGroup))
            {
                i += 1;
            }
            i = skip_ws(input, i);
        }
        let Some(source) = input.get(i).cloned() else {
            return Ok(None);
        };
        if matches!(source, Token::EndGroup) {
            return Ok(None);
        }
        let meaning = match &source {
            Token::ControlWord(name) => self
                .lookup_macro(name)
                .unwrap_or_else(|| Meaning::Token(source.clone())),
            Token::ControlSymbol(name) => Meaning::Token(Token::ControlSymbol(*name)),
            _ => Meaning::Token(source),
        };
        self.define_macro(target, meaning, global);
        Ok(Some(i + 1))
    }

    fn parse_newcommand(
        &mut self,
        input: &[Token],
        start: usize,
        command: &str,
        _index: &mut usize,
    ) -> Result<Option<usize>, usize> {
        let mut i = start + 1;
        i = skip_ws(input, i);
        if matches!(input.get(i), Some(Token::ControlSymbol('*'))) {
            i += 1;
            i = skip_ws(input, i);
        }
        let (after_name, name) = if let Some((end, inner)) = group_at(input, i) {
            let Some((_, name)) = control_word_at(&inner, skip_ws(&inner, 0)) else {
                return Ok(None);
            };
            (end, name)
        } else if let Some((end, name)) = control_word_at(input, i) {
            (end, name)
        } else {
            return Ok(None);
        };
        i = skip_ws(input, after_name);

        let mut param_count = 0u8;
        if let Some((end, optional_count)) = bracket_at(input, i) {
            let text = token_text(&optional_count);
            let Ok(n) = text.trim().parse::<u8>() else {
                return Ok(None);
            };
            param_count = n;
            i = skip_ws(input, end);
        }
        let mut optional_default = None;
        if param_count > 0
            && let Some((end, default)) = bracket_at(input, i)
        {
            optional_default = Some(default);
            i = skip_ws(input, end);
        }
        let Some((end, body)) = group_at(input, i) else {
            return Ok(None);
        };
        if command != "providecommand" || self.lookup_macro(&name).is_none() {
            self.define_macro(
                name,
                Meaning::Macro(MacroDef {
                    param_count,
                    optional_default,
                    body,
                }),
                false,
            );
        }
        Ok(Some(end))
    }

    fn parse_newenvironment(
        &mut self,
        input: &[Token],
        start: usize,
        _index: &mut usize,
    ) -> Result<Option<usize>, usize> {
        let mut i = skip_ws(input, start + 1);
        if matches!(input.get(i), Some(Token::ControlSymbol('*'))) {
            i = skip_ws(input, i + 1);
        }
        let Some((after_name, name_tokens)) = group_at(input, i) else {
            return Ok(None);
        };
        i = skip_ws(input, after_name);
        if let Some((end, _)) = bracket_at(input, i) {
            i = skip_ws(input, end);
        }
        let Some((after_begin, begin)) = group_at(input, i) else {
            return Ok(None);
        };
        let Some((end, finish)) = group_at(input, skip_ws(input, after_begin)) else {
            return Ok(None);
        };
        let name = token_text(&name_tokens).trim().to_owned();
        self.define_environment(name, EnvDef { begin, end: finish }, false);
        Ok(Some(end))
    }

    /// Expand an environment introduced by `\newenvironment`.
    fn process_environment_command(
        &mut self,
        input: &[Token],
        index: &mut usize,
        out: &mut Vec<Token>,
        expansion_depth: usize,
    ) -> Result<bool, usize> {
        let Token::ControlWord(command) = &input[*index] else {
            return Ok(false);
        };
        if command != "begin" && command != "end" {
            return Ok(false);
        }
        let after_command = skip_ws(input, *index + 1);
        let Some((after_name, name_tokens)) = group_at(input, after_command) else {
            return Ok(false);
        };
        let name = token_text(&name_tokens).trim().to_owned();
        let Some(env) = self.lookup_environment(&name) else {
            return Ok(false);
        };
        let body = if command == "begin" { env.begin } else { env.end };
        self.process_tokens(&body, out, Mode::Normal, expansion_depth + 1)?;
        if command == "begin" {
            self.environment_depth = self.environment_depth.saturating_add(1);
        } else if self.environment_depth > 0 {
            self.environment_depth -= 1;
        }
        *index = after_name;
        Ok(true)
    }

    /// Expand a regular macro or a `\let` meaning at `*index`.
    fn process_macro_command(
        &mut self,
        input: &[Token],
        index: &mut usize,
        out: &mut Vec<Token>,
        expansion_depth: usize,
    ) -> Result<bool, usize> {
        let Token::ControlWord(name) = &input[*index] else {
            return Ok(false);
        };
        let Some(meaning) = self.lookup_macro(name) else {
            return Ok(false);
        };
        if expansion_depth >= MAX_EXPANSION_DEPTH {
            self.append(out, input[*index].clone())?;
            *index += 1;
            return Ok(true);
        }
        match meaning {
            Meaning::Macro(def) => {
                let Some((after_args, args)) = consume_args(input, *index + 1, &def) else {
                    return Ok(false);
                };
                let replacement = substitute(&def.body, &args);
                self.process_tokens(&replacement, out, Mode::Normal, expansion_depth + 1)?;
                *index = after_args;
                Ok(true)
            },
            Meaning::Token(token) => {
                let emitted = match token {
                    Token::ControlWord(_) | Token::ControlSymbol(_) => {
                        Token::Frozen(Box::new(token))
                    },
                    other => other,
                };
                self.append(out, emitted)?;
                *index += 1;
                Ok(true)
            },
        }
    }

    fn process_noexpand(
        &mut self,
        input: &[Token],
        index: &mut usize,
        out: &mut Vec<Token>,
    ) -> Result<(), usize> {
        let next = skip_ws(input, *index + 1);
        if next >= input.len() {
            self.append(out, input[*index].clone())?;
            *index += 1;
            return Ok(());
        }
        let token = input[next].clone();
        if let Token::ControlWord(name) = &token
            && let Some(Meaning::Token(meaning)) = self.lookup_macro(name)
        {
            self.append(out, meaning)?;
        } else {
            self.append(out, Token::Frozen(Box::new(token)))?;
        }
        *index = next + 1;
        Ok(())
    }

    /// Expand only expandable macros while constructing an `\edef` body.
    fn process_edef_command(
        &mut self,
        input: &[Token],
        index: &mut usize,
        out: &mut Vec<Token>,
        expansion_depth: usize,
    ) -> Result<bool, usize> {
        let Token::ControlWord(name) = &input[*index] else {
            return Ok(false);
        };
        if name == "noexpand" {
            self.process_edef_noexpand(input, index, out)?;
            return Ok(true);
        }
        let Some(Meaning::Macro(def)) = self.lookup_macro(name) else {
            return Ok(false);
        };
        if expansion_depth >= MAX_EXPANSION_DEPTH {
            self.append(out, input[*index].clone())?;
            *index += 1;
            return Ok(true);
        }
        let Some((after_args, args)) = consume_args(input, *index + 1, &def) else {
            return Ok(false);
        };
        let replacement = substitute(&def.body, &args);
        self.process_tokens(&replacement, out, Mode::Edef, expansion_depth + 1)?;
        *index = after_args;
        Ok(true)
    }

    fn process_edef_noexpand(
        &mut self,
        input: &[Token],
        index: &mut usize,
        out: &mut Vec<Token>,
    ) -> Result<(), usize> {
        let next = skip_ws(input, *index + 1);
        if next >= input.len() {
            self.append(out, input[*index].clone())?;
            *index += 1;
            return Ok(());
        }
        // In an edef, \noexpand removes itself but preserves the following
        // token's identity.  The token is intentionally not resolved through
        // \let here: a later redefinition must still be visible when the edef
        // body is called.
        self.append(out, input[next].clone())?;
        *index = next + 1;
        Ok(())
    }

    fn wrap_text_macro_argument(
        &mut self,
        input: &[Token],
        index: &mut usize,
        out: &mut Vec<Token>,
        expansion_depth: usize,
    ) -> Result<bool, usize> {
        let Token::ControlWord(name) = &input[*index] else {
            return Ok(false);
        };
        if text_style(name).is_none()
            && !matches!(name.as_str(), "angl" | "reflectbox" | "mathreflectbox")
        {
            return Ok(false);
        }
        let next = skip_ws(input, *index + 1);
        let Some(Token::ControlWord(argument)) = input.get(next) else {
            return Ok(false);
        };
        if !matches!(self.lookup_macro(argument), Some(Meaning::Macro(_))) {
            return Ok(false);
        }
        self.append(out, input[*index].clone())?;
        self.append(out, Token::BeginGroup)?;
        let mut one = next;
        if !self.process_macro_command(input, &mut one, out, expansion_depth)? {
            return Ok(false);
        }
        self.append(out, Token::EndGroup)?;
        *index = one;
        Ok(true)
    }
}

fn skip_ws(input: &[Token], mut index: usize) -> usize {
    while index < input.len() && input[index].is_whitespace() {
        index += 1;
    }
    index
}

fn control_word_at(input: &[Token], index: usize) -> Option<(usize, String)> {
    match input.get(index)? {
        Token::ControlWord(name) => Some((index + 1, name.clone())),
        _ => None,
    }
}

fn group_at(input: &[Token], index: usize) -> Option<(usize, Vec<Token>)> {
    let index = skip_ws(input, index);
    if !matches!(input.get(index), Some(Token::BeginGroup)) {
        return None;
    }
    let mut depth = 1usize;
    let mut i = index + 1;
    while i < input.len() {
        match input[i] {
            Token::BeginGroup => depth += 1,
            Token::EndGroup => {
                depth -= 1;
                if depth == 0 {
                    return Some((i + 1, input[index + 1..i].to_vec()));
                }
            },
            _ => {},
        }
        i += 1;
    }
    None
}

fn bracket_at(input: &[Token], index: usize) -> Option<(usize, Vec<Token>)> {
    let index = skip_ws(input, index);
    if !matches!(input.get(index), Some(Token::Char('['))) {
        return None;
    }
    let mut braces = 0usize;
    let mut i = index + 1;
    while i < input.len() {
        match input[i] {
            Token::BeginGroup => braces += 1,
            Token::EndGroup => braces = braces.saturating_sub(1),
            Token::Char(']') if braces == 0 => {
                return Some((i + 1, input[index + 1..i].to_vec()));
            },
            _ => {},
        }
        i += 1;
    }
    None
}

fn consume_args<'a>(
    input: &'a [Token],
    start: usize,
    def: &MacroDef,
) -> Option<(usize, Vec<Vec<Token>>)> {
    if def.param_count == 0 {
        return Some((start, Vec::new()));
    }
    let mut i = start;
    let mut args = Vec::with_capacity(def.param_count as usize);
    if let Some(default) = &def.optional_default {
        if let Some((end, value)) = bracket_at(input, i) {
            args.push(value);
            i = end;
        } else {
            args.push(default.clone());
        }
    }
    while args.len() < def.param_count as usize {
        let next = skip_ws(input, i);
        if let Some((end, value)) = group_at(input, next) {
            args.push(value);
            i = end;
        } else {
            let token = input.get(next)?.clone();
            if matches!(token, Token::EndGroup) {
                return None;
            }
            args.push(vec![token]);
            i = next + 1;
        }
    }
    Some((i, args))
}

fn substitute(body: &[Token], args: &[Vec<Token>]) -> Vec<Token> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if matches!(body[i], Token::Char('#'))
            && let Some(Token::Char(digit)) = body.get(i + 1)
            && let Some(index) = digit.to_digit(10)
            && index > 0
            && (index as usize) <= args.len()
        {
            out.extend(args[index as usize - 1].iter().cloned());
            i += 2;
            continue;
        }
        out.push(body[i].clone());
        i += 1;
    }
    out
}

fn token_text(tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        match token {
            Token::Char(c) => out.push(*c),
            Token::BeginGroup => out.push('{'),
            Token::EndGroup => out.push('}'),
            Token::ControlWord(name) => {
                out.push('\\');
                out.push_str(name);
            },
            Token::ControlSymbol(c) => {
                out.push('\\');
                out.push(*c);
            },
            Token::Verbatim(raw) => out.push_str(raw),
            Token::Frozen(inner) => out.push_str(&token_text(std::slice::from_ref(inner))),
        }
    }
    out
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::with_capacity(input.len().min(4096));
    let mut i = 0;
    while i < input.len() {
        if input.as_bytes()[i] == b'\\' {
            if let Some(end) = find_verb_end(input, i) {
                tokens.push(Token::Verbatim(input[i..end].to_owned()));
                i = end;
                continue;
            }
            let after = i + 1;
            if after >= input.len() {
                tokens.push(Token::ControlSymbol('\\'));
                break;
            }
            let first = input[after..].chars().next().unwrap();
            if first.is_ascii_alphabetic() {
                let name_len: usize = input[after..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .map(char::len_utf8)
                    .sum();
                tokens.push(Token::ControlWord(input[after..after + name_len].to_owned()));
                i = after + name_len;
            } else {
                tokens.push(Token::ControlSymbol(first));
                i = after + first.len_utf8();
            }
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        i += ch.len_utf8();
        tokens.push(match ch {
            '{' => Token::BeginGroup,
            '}' => Token::EndGroup,
            _ => Token::Char(ch),
        });
    }
    tokens
}

fn render_tokens(tokens: &[Token]) -> Result<String, ()> {
    let mut out = String::new();
    for token in tokens {
        let text = match token {
            Token::Char(c) => c.to_string(),
            Token::BeginGroup => "{".to_owned(),
            Token::EndGroup => "}".to_owned(),
            Token::ControlWord(name) => format!("\\{name}"),
            Token::ControlSymbol(c) => format!("\\{c}"),
            Token::Verbatim(raw) => raw.clone(),
            Token::Frozen(inner) => {
                let rendered = render_tokens(std::slice::from_ref(inner))?;
                rendered
            },
        };
        out.push_str(&text);
    }
    Ok(out)
}

