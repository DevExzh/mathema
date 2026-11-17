//! Ordered/scoped macro expansion over an explicit token stream.
//!
//! Replacements are pushed on the front of a `VecDeque` while the unconsumed
//! input remains behind them.  Consequently a macro at the end of a
//! replacement can consume arguments from the outer input, and a selector can
//! discard a branch before any of its tokens execute.  The token type and the
//! processor are crate-visible so the conditional/selection helper can attach
//! to this stream without reparsing rendered strings.

use std::collections::{HashMap, VecDeque};

use super::packages;
use super::{expansion_cap, find_verb_end};
use crate::latex::text_style;

const MAX_EXPANSION_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Char(char),
    BeginGroup,
    EndGroup,
    ControlWord(String),
    ControlSymbol(char),
    Verbatim(String),
    /// A frozen control-sequence token is emitted literally.  It carries the
    /// identity captured by `\let` or by ordinary `\noexpand` handling.
    Frozen(Box<Token>),
}

impl Token {
    fn is_whitespace(&self) -> bool {
        matches!(self, Self::Char(c) if c.is_ascii_whitespace())
    }

    fn byte_len(&self) -> usize {
        match self {
            Self::Char(c) => c.len_utf8(),
            Self::BeginGroup | Self::EndGroup => 1,
            Self::ControlWord(name) => name.len() + 1,
            Self::ControlSymbol(c) => c.len_utf8() + 1,
            Self::Verbatim(text) => text.len(),
            Self::Frozen(token) => token.byte_len(),
        }
    }
}

#[derive(Clone, Debug)]
struct Queued {
    token: Token,
    depth: usize,
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
    Edef,
}

pub(crate) fn expand(input: &str) -> Result<String, usize> {
    let cap = expansion_cap(input.len());
    let mut processor = Processor::new(cap);
    processor.seed_builtins();
    let mut queue: VecDeque<Queued> = tokenize(input)
        .into_iter()
        .map(|token| Queued { token, depth: 0 })
        .collect();
    let mut output = Vec::with_capacity(input.len());
    processor.process_queue(&mut queue, &mut output, Mode::Normal)?;
    Ok(render_tokens(&output))
}

pub(crate) struct Processor {
    scopes: Vec<Scope>,
    cap: usize,
    produced: usize,
    environment_scopes: usize,
}

impl Processor {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            scopes: vec![Scope::default()],
            cap,
            produced: 0,
            environment_scopes: 0,
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
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn define_macro(&mut self, name: String, meaning: Meaning, global: bool) {
        if global {
            // A global assignment overrides local meanings immediately at all
            // active levels, as in TeX (`\gdef` must be visible before each
            // enclosing group closes).
            for scope in self.scopes.iter_mut().skip(1) {
                scope.macros.remove(&name);
            }
            self.scopes[0].macros.insert(name, meaning);
        } else {
            let scope = self.scopes.len() - 1;
            self.scopes[scope].macros.insert(name, meaning);
        }
    }

    fn define_environment(&mut self, name: String, env: EnvDef, global: bool) {
        if global {
            for scope in self.scopes.iter_mut().skip(1) {
                scope.environments.remove(&name);
            }
            self.scopes[0].environments.insert(name, env);
        } else {
            let scope = self.scopes.len() - 1;
            self.scopes[scope].environments.insert(name, env);
        }
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

    fn process_queue(
        &mut self,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
        mode: Mode,
    ) -> Result<(), usize> {
        while let Some(Queued { token, depth }) = queue.pop_front() {
            match token {
                Token::BeginGroup => {
                    self.append(out, Token::BeginGroup)?;
                    self.push_scope();
                },
                Token::EndGroup => {
                    self.append(out, Token::EndGroup)?;
                    self.pop_scope();
                },
                Token::Verbatim(text) => {
                    self.append(out, Token::Verbatim(text))?;
                },
                Token::ControlWord(name) => {
                    if name == "__mathema_end_environment" {
                        self.pop_scope();
                        self.environment_scopes = self.environment_scopes.saturating_sub(1);
                        continue;
                    }
                    if mode == Mode::Edef {
                        if name == "noexpand" {
                            self.edef_noexpand(queue, out)?;
                        } else if !self.expand_macro_from_queue(&name, depth, queue, out, Mode::Edef)?
                        {
                            self.append(out, Token::ControlWord(name))?;
                        }
                        continue;
                    }

                    if self.definition_command(&name, queue)? {
                        continue;
                    }
                    if self.environment_command(&name, depth, queue)? {
                        continue;
                    }
                    if name == "noexpand" {
                        self.noexpand(queue, out)?;
                    } else if !self.expand_macro_from_queue(
                        &name,
                        depth,
                        queue,
                        out,
                        Mode::Normal,
                    )? {
                        // Native text commands consume one token.  Grouping a
                        // bare macro here preserves the parser's text mode.
                        if !self.wrap_text_argument(&name, depth, queue, out)? {
                            self.append(out, Token::ControlWord(name))?;
                        }
                    }
                },
                Token::ControlSymbol(symbol) => {
                    self.append(out, Token::ControlSymbol(symbol))?;
                },
                Token::Char(ch) => {
                    self.append(out, Token::Char(ch))?;
                },
                Token::Frozen(token) => {
                    self.append(out, *token)?;
                },
            }
        }
        Ok(())
    }

    fn expand_macro_from_queue(
        &mut self,
        name: &str,
        depth: usize,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
        mode: Mode,
    ) -> Result<bool, usize> {
        let Some(meaning) = self.lookup_macro(name) else {
            return Ok(false);
        };
        if depth >= MAX_EXPANSION_DEPTH {
            self.append(out, Token::ControlWord(name.to_owned()))?;
            return Ok(true);
        }
        match meaning {
            Meaning::Macro(def) => {
                let Some(args) = consume_args(queue, &def) else {
                    return Ok(false);
                };
                let replacement = substitute(&def.body, &args, self.cap)?;
                prepend(queue, replacement, depth + 1);
                // In edef mode the newly enqueued body must be expanded by the
                // same mode; the main loop carries that mode through naturally.
                let _ = mode;
                Ok(true)
            },
            Meaning::Token(token) => {
                if mode == Mode::Edef {
                    // A `\let` token is unexpandable during an edef.  Keep
                    // its control-sequence identity so a later redefinition
                    // of the let target is still observed when the edef body
                    // is invoked.
                    self.append(out, Token::ControlWord(name.to_owned()))?;
                    return Ok(true);
                }
                let token = match token {
                    Token::ControlWord(_) | Token::ControlSymbol(_) => {
                        Token::Frozen(Box::new(token))
                    },
                    other => other,
                };
                self.append(out, token)?;
                Ok(true)
            },
        }
    }

    fn definition_command(
        &mut self,
        name: &str,
        queue: &mut VecDeque<Queued>,
    ) -> Result<bool, usize> {
        if matches!(name, "def" | "gdef" | "edef" | "xdef") {
            let mut txn = QueueTxn::new(queue);
            let result = self.parse_def_txn(
                name,
                false,
                matches!(name, "edef" | "xdef"),
                &mut txn,
            )?;
            return Ok(if result { txn.commit(); true } else { txn.rollback(); false });
        }
        if name == "let" {
            let mut txn = QueueTxn::new(queue);
            let result = self.parse_let_txn(false, &mut txn)?;
            return Ok(if result { txn.commit(); true } else { txn.rollback(); false });
        }
        if matches!(name, "newcommand" | "renewcommand" | "providecommand") {
            let mut txn = QueueTxn::new(queue);
            let result = self.parse_newcommand_txn(name, &mut txn)?;
            return Ok(if result { txn.commit(); true } else { txn.rollback(); false });
        }
        if name == "newenvironment" {
            let mut txn = QueueTxn::new(queue);
            let result = self.parse_newenvironment_txn(&mut txn)?;
            return Ok(if result { txn.commit(); true } else { txn.rollback(); false });
        }
        if !matches!(name, "global" | "long") {
            return Ok(false);
        }

        // Prefix parsing is transactional.  If the next command is not a
        // definition primitive, restoring it leaves `\global` in the output
        // for the strict parser to reject.
        let mut txn = QueueTxn::new(queue);
        let mut global = name == "global";
        loop {
            let Some(next_name) = txn.peek_control_word() else {
                txn.rollback();
                return Ok(false);
            };
            if next_name == "global" {
                global = true;
                txn.pop();
                continue;
            }
            if next_name == "long" {
                txn.pop();
                continue;
            }
            break;
        }
        let Some(command) = txn.peek_control_word() else {
            txn.rollback();
            return Ok(false);
        };
        if matches!(command.as_str(), "def" | "gdef" | "edef" | "xdef") {
            txn.pop();
            let result = self.parse_def_txn(
                &command,
                global || command == "gdef" || command == "xdef",
                matches!(command.as_str(), "edef" | "xdef"),
                &mut txn,
            )?;
            return Ok(if result { txn.commit(); true } else { txn.rollback(); false });
        }
        if command == "let" {
            txn.pop();
            let result = self.parse_let_txn(true, &mut txn)?;
            return Ok(if result { txn.commit(); true } else { txn.rollback(); false });
        }
        // TeX permits a macro expanding to `\def` after a prefix.
        if let Some(Meaning::Macro(def)) = self.lookup_macro(&command)
            && def.param_count == 0
            && def.body.len() == 1
            && let Token::ControlWord(effective) = &def.body[0]
            && matches!(effective.as_str(), "def" | "gdef" | "edef" | "xdef")
        {
            txn.pop();
            let result = self.parse_def_txn(
                effective,
                global || effective == "gdef" || effective == "xdef",
                matches!(effective.as_str(), "edef" | "xdef"),
                &mut txn,
            )?;
            return Ok(if result { txn.commit(); true } else { txn.rollback(); false });
        }
        txn.rollback();
        Ok(false)
    }

    fn parse_def_txn(
        &mut self,
        command: &str,
        global: bool,
        expand_body: bool,
        txn: &mut QueueTxn<'_>,
    ) -> Result<bool, usize> {
        let Some(target) = txn.pop_control_word() else {
            return Ok(false);
        };
        let mut count = 0u8;
        loop {
            txn.skip_whitespace();
            let Some(hash) = txn.peek_token() else {
                return Ok(false);
            };
            if !matches!(hash, Token::Char('#')) {
                break;
            }
            txn.pop();
            let Some(Token::Char(digit)) = txn.pop_token() else {
                return Ok(false);
            };
            let n = digit.to_digit(10).unwrap_or(0);
            if n == 0 || n > 9 || n != u32::from(count) + 1 {
                // Delimited/malformed parameter text is left to the dedicated
                // argument scanner instead of being silently accepted here.
                return Ok(false);
            }
            count += 1;
        }
        let Some(body) = txn.pop_group() else {
            return Ok(false);
        };
        let body = if expand_body {
            self.expand_edef_body(body)?
        } else {
            body
        };
        self.define_macro(
            target,
            Meaning::Macro(MacroDef {
                param_count: count,
                optional_default: None,
                body,
            }),
            global || command == "gdef" || command == "xdef",
        );
        Ok(true)
    }

    fn parse_let_txn(
        &mut self,
        global: bool,
        txn: &mut QueueTxn<'_>,
    ) -> Result<bool, usize> {
        let Some(target) = txn.pop_control_word() else {
            return Ok(false);
        };
        txn.skip_whitespace();
        if matches!(txn.peek_token(), Some(Token::Char('='))) {
            txn.pop();
            if matches!(txn.peek_token(), Some(Token::Char(' ')))
                && !matches!(txn.peek_token_at(1), Some(Token::EndGroup))
            {
                txn.pop();
            }
            txn.skip_whitespace();
        }
        let Some(source) = txn.pop_token() else {
            return Ok(false);
        };
        if matches!(source, Token::EndGroup) {
            return Ok(false);
        }
        let meaning = match &source {
            Token::ControlWord(name) => self
                .lookup_macro(name)
                .unwrap_or_else(|| Meaning::Token(source.clone())),
            Token::ControlSymbol(symbol) => Meaning::Token(Token::ControlSymbol(*symbol)),
            _ => Meaning::Token(source),
        };
        self.define_macro(target, meaning, global);
        Ok(true)
    }

    fn parse_newcommand_txn(
        &mut self,
        command: &str,
        txn: &mut QueueTxn<'_>,
    ) -> Result<bool, usize> {
        txn.skip_whitespace();
        if matches!(txn.peek_token(), Some(Token::ControlSymbol('*'))) {
            txn.pop();
            txn.skip_whitespace();
        }
        let Some(target) = txn.pop_command_name_or_group() else {
            return Ok(false);
        };
        txn.skip_whitespace();
        let mut count = 0u8;
        if let Some(number) = txn.pop_bracket() {
            let text = token_text(&number);
            let Ok(parsed) = text.trim().parse::<u8>() else {
                return Ok(false);
            };
            count = parsed;
            txn.skip_whitespace();
        }
        let default = if count > 0 {
            let default = txn.pop_bracket();
            if default.is_some() {
                txn.skip_whitespace();
            }
            default
        } else {
            None
        };
        let Some(body) = txn.pop_group() else {
            return Ok(false);
        };
        if command != "providecommand" || self.lookup_macro(&target).is_none() {
            self.define_macro(
                target,
                Meaning::Macro(MacroDef {
                    param_count: count,
                    optional_default: default,
                    body,
                }),
                false,
            );
        }
        Ok(true)
    }

    fn parse_newenvironment_txn(&mut self, txn: &mut QueueTxn<'_>) -> Result<bool, usize> {
        txn.skip_whitespace();
        if matches!(txn.peek_token(), Some(Token::ControlSymbol('*'))) {
            txn.pop();
            txn.skip_whitespace();
        }
        let Some(name) = txn.pop_group() else {
            return Ok(false);
        };
        txn.skip_whitespace();
        if txn.peek_token_is('[') {
            let _ = txn.pop_bracket();
            txn.skip_whitespace();
        }
        let Some(begin) = txn.pop_group() else {
            return Ok(false);
        };
        let Some(end) = txn.pop_group() else {
            return Ok(false);
        };
        self.define_environment(
            token_text(&name).trim().to_owned(),
            EnvDef { begin, end },
            false,
        );
        Ok(true)
    }

    fn environment_command(
        &mut self,
        name: &str,
        depth: usize,
        queue: &mut VecDeque<Queued>,
    ) -> Result<bool, usize> {
        if name != "begin" && name != "end" {
            return Ok(false);
        }
        let mut txn = QueueTxn::new(queue);
        txn.skip_whitespace();
        let Some(group) = txn.pop_group() else {
            txn.rollback();
            return Ok(false);
        };
        let env_name = token_text(&group).trim().to_owned();
        let Some(env) = self.lookup_environment(&env_name) else {
            txn.rollback();
            return Ok(false);
        };
        txn.commit();
        drop(txn);
        if name == "begin" {
            self.push_scope();
            self.environment_scopes += 1;
            prepend(queue, env.begin, depth + 1);
        } else {
            if self.environment_scopes > 0 {
                // The end body must run before the environment scope is
                // restored; place a private marker after it.
                queue.push_front(Queued {
                    token: Token::ControlWord("__mathema_end_environment".to_owned()),
                    depth,
                });
            }
            prepend(queue, env.end, depth + 1);
        }
        Ok(true)
    }

    fn noexpand(
        &mut self,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
    ) -> Result<(), usize> {
        let mut txn = QueueTxn::new(queue);
        txn.skip_whitespace();
        let Some(token) = txn.pop_token() else {
            txn.rollback();
            self.append(out, Token::ControlWord("noexpand".to_owned()))?;
            return Ok(());
        };
        txn.commit();
        if let Token::ControlWord(name) = &token
            && let Some(Meaning::Token(meaning)) = self.lookup_macro(name)
        {
            self.append(out, meaning)?;
        } else {
            self.append(out, Token::Frozen(Box::new(token)))?;
        }
        Ok(())
    }

    fn edef_noexpand(
        &mut self,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
    ) -> Result<(), usize> {
        let mut txn = QueueTxn::new(queue);
        txn.skip_whitespace();
        let Some(token) = txn.pop_token() else {
            txn.rollback();
            self.append(out, Token::ControlWord("noexpand".to_owned()))?;
            return Ok(());
        };
        txn.commit();
        self.append(out, token)
    }

    fn wrap_text_argument(
        &mut self,
        name: &str,
        depth: usize,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
    ) -> Result<bool, usize> {
        if text_style(name).is_none()
            && !matches!(name, "angl" | "reflectbox" | "mathreflectbox")
        {
            return Ok(false);
        }
        let Some(argument) = queue.iter().find(|item| !item.token.is_whitespace()) else {
            return Ok(false);
        };
        let Token::ControlWord(argument_name) = &argument.token else {
            return Ok(false);
        };
        if !matches!(self.lookup_macro(argument_name), Some(Meaning::Macro(_))) {
            return Ok(false);
        }
        // Remove separator whitespace and the macro token from the queue, then
        // put the token back inside a generated group.  The group token drives
        // scope restoration through the same main loop as source braces.
        while matches!(queue.front(), Some(item) if item.token.is_whitespace()) {
            queue.pop_front();
        }
        let Some(argument_item) = queue.pop_front() else {
            return Ok(false);
        };
        self.append(out, Token::ControlWord(name.to_owned()))?;
        self.append(out, Token::BeginGroup)?;
        self.push_scope();
        queue.push_front(Queued {
            token: Token::EndGroup,
            depth,
        });
        queue.push_front(argument_item);
        Ok(true)
    }

    fn expand_edef_body(&mut self, body: Vec<Token>) -> Result<Vec<Token>, usize> {
        let initial_scopes = self.scopes.len();
        let mut queue: VecDeque<Queued> = body
            .into_iter()
            .map(|token| Queued { token, depth: 0 })
            .collect();
        let mut out = Vec::new();
        let result = self.process_queue(&mut queue, &mut out, Mode::Edef);
        self.scopes.truncate(initial_scopes);
        result.map(|()| out)
    }
}

struct QueueTxn<'a> {
    queue: &'a mut VecDeque<Queued>,
    taken: Vec<Queued>,
    committed: bool,
}

impl<'a> QueueTxn<'a> {
    fn new(queue: &'a mut VecDeque<Queued>) -> Self {
        Self {
            queue,
            taken: Vec::new(),
            committed: false,
        }
    }

    fn pop(&mut self) -> Option<Queued> {
        let item = self.queue.pop_front()?;
        self.taken.push(item.clone());
        Some(item)
    }

    fn pop_token(&mut self) -> Option<Token> {
        self.pop().map(|item| item.token)
    }

    fn peek_token(&self) -> Option<&Token> {
        self.queue.front().map(|item| &item.token)
    }

    fn peek_token_at(&self, offset: usize) -> Option<&Token> {
        self.queue.get(offset).map(|item| &item.token)
    }

    fn peek_control_word(&self) -> Option<String> {
        match self.peek_token()? {
            Token::ControlWord(name) => Some(name.clone()),
            _ => None,
        }
    }

    fn pop_control_word(&mut self) -> Option<String> {
        match self.pop_token()? {
            Token::ControlWord(name) => Some(name),
            _ => None,
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_token(), Some(token) if token.is_whitespace()) {
            self.pop();
        }
    }

    fn peek_token_is(&self, ch: char) -> bool {
        matches!(self.peek_token(), Some(Token::Char(value)) if *value == ch)
    }

    fn pop_command_name_or_group(&mut self) -> Option<String> {
        self.skip_whitespace();
        if matches!(self.peek_token(), Some(Token::BeginGroup)) {
            let inner = self.pop_group()?;
            let mut i = 0;
            while i < inner.len() && inner[i].is_whitespace() {
                i += 1;
            }
            return match inner.get(i)? {
                Token::ControlWord(name) => Some(name.clone()),
                _ => None,
            };
        }
        self.pop_control_word()
    }

    fn pop_group(&mut self) -> Option<Vec<Token>> {
        self.skip_whitespace();
        if !matches!(self.peek_token(), Some(Token::BeginGroup)) {
            return None;
        }
        self.pop();
        let mut depth = 1usize;
        let mut inner = Vec::new();
        while let Some(token) = self.pop_token() {
            match token {
                Token::BeginGroup => {
                    depth += 1;
                    inner.push(Token::BeginGroup);
                },
                Token::EndGroup => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(inner);
                    }
                    inner.push(Token::EndGroup);
                },
                other => inner.push(other),
            }
        }
        None
    }

    fn pop_bracket(&mut self) -> Option<Vec<Token>> {
        self.skip_whitespace();
        if !matches!(self.peek_token(), Some(Token::Char('['))) {
            return None;
        }
        self.pop();
        let mut braces = 0usize;
        let mut inner = Vec::new();
        while let Some(token) = self.pop_token() {
            match token {
                Token::BeginGroup => {
                    braces += 1;
                    inner.push(Token::BeginGroup);
                },
                Token::EndGroup => {
                    braces = braces.saturating_sub(1);
                    inner.push(Token::EndGroup);
                },
                Token::Char(']') if braces == 0 => return Some(inner),
                other => inner.push(other),
            }
        }
        None
    }

    fn commit(&mut self) {
        self.committed = true;
        self.taken.clear();
    }

    fn rollback(&mut self) {
        if self.committed {
            return;
        }
        for item in self.taken.drain(..).rev() {
            self.queue.push_front(item);
        }
    }
}

impl Drop for QueueTxn<'_> {
    fn drop(&mut self) {
        if !self.committed {
            for item in self.taken.drain(..).rev() {
                self.queue.push_front(item);
            }
        }
    }
}

fn consume_args(queue: &mut VecDeque<Queued>, def: &MacroDef) -> Option<Vec<Vec<Token>>> {
    let mut txn = QueueTxn::new(queue);
    let mut args = Vec::with_capacity(def.param_count as usize);
    if let Some(default) = &def.optional_default {
        if let Some(value) = txn.pop_bracket() {
            args.push(value);
        } else {
            args.push(default.clone());
        }
    }
    while args.len() < def.param_count as usize {
        txn.skip_whitespace();
        let value = if matches!(txn.peek_token(), Some(Token::BeginGroup)) {
            txn.pop_group()?
        } else {
            let token = txn.pop_token()?;
            if matches!(token, Token::EndGroup) {
                return None;
            }
            vec![token]
        };
        args.push(value);
    }
    txn.commit();
    Some(args)
}

fn substitute(body: &[Token], args: &[Vec<Token>], cap: usize) -> Result<Vec<Token>, usize> {
    let mut size = 0usize;
    let mut i = 0;
    while i < body.len() {
        if matches!(body[i], Token::Char('#'))
            && let Some(Token::Char(digit)) = body.get(i + 1)
            && let Some(index) = digit.to_digit(10)
            && index > 0
            && (index as usize) <= args.len()
        {
            for token in &args[index as usize - 1] {
                size = size.saturating_add(token.byte_len());
                if size > cap {
                    return Err(cap);
                }
            }
            i += 2;
            continue;
        }
        size = size.saturating_add(body[i].byte_len());
        if size > cap {
            return Err(cap);
        }
        i += 1;
    }
    let mut out = Vec::with_capacity(body.len().saturating_add(size.min(cap)));
    i = 0;
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
    Ok(out)
}

fn prepend(queue: &mut VecDeque<Queued>, tokens: Vec<Token>, depth: usize) {
    for token in tokens.into_iter().rev() {
        queue.push_front(Queued { token, depth });
    }
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
            Token::Verbatim(text) => out.push_str(text),
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

fn render_tokens(tokens: &[Token]) -> String {
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
            Token::Verbatim(text) => out.push_str(text),
            Token::Frozen(inner) => out.push_str(&render_tokens(std::slice::from_ref(inner))),
        }
    }
    out
}

