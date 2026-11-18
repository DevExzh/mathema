//! Ordered/scoped macro expansion over an explicit token stream.
//!
//! Replacements are pushed on the front of a `VecDeque` while the unconsumed
//! input remains behind them.  Consequently a macro at the end of a
//! replacement can consume arguments from the outer input, and a selector can
//! discard a branch before any of its tokens execute.  The token type and the
//! processor are crate-visible so the conditional/selection helper can attach
//! to this stream without reparsing rendered strings.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use super::packages;
use super::{
    ExpansionFailure, SyntaxCommand, SyntaxKind, expansion_cap, expansion_work_cap, find_verb_end,
};
use crate::latex::macro_args::{self, ParameterText};
use crate::latex::text_style;

#[path = "ordered_select.rs"]
mod select;

type ExpansionResult<T> = std::result::Result<T, ExpansionFailure>;

const MAX_EXPANSION_DEPTH: usize = 64;

fn malformed(command: &str, kind: SyntaxKind) -> ExpansionFailure {
    ExpansionFailure::Syntax {
        command: SyntaxCommand::from_name(command),
        kind,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Char(char),
    BeginGroup,
    EndGroup,
    ControlWord(String),
    ControlSymbol(char),
    Verbatim(String),
    EndEnvironment,
    /// A token protected by ordinary `\noexpand`.  The protected token is
    /// treated as `\relax` when it reaches the stream; this is distinct from
    /// [`Frozen`], which preserves an unexpandable token's meaning.
    NoExpand(Box<Token>),
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
            Self::EndEnvironment => 0,
            Self::NoExpand(token) => token.byte_len(),
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
    body: Arc<[Token]>,
    parameter_text: Option<ParameterText>,
    protected: bool,
}

struct BuiltinMacro {
    name: &'static str,
    param_count: u8,
    body: Arc<[Token]>,
}

fn tokenized_builtin_macros() -> &'static [BuiltinMacro] {
    static BUILTINS: OnceLock<Vec<BuiltinMacro>> = OnceLock::new();
    BUILTINS
        .get_or_init(|| {
            packages::builtin_macros()
                .map(|(name, param_count, body)| BuiltinMacro {
                    name,
                    param_count,
                    body: Arc::from(tokenize(body)),
                })
                .collect()
        })
        .as_slice()
}

fn builtin_meanings() -> &'static HashMap<&'static str, Meaning> {
    static BUILTINS: OnceLock<HashMap<&'static str, Meaning>> = OnceLock::new();
    BUILTINS.get_or_init(|| {
        let mut meanings = HashMap::new();
        for builtin in tokenized_builtin_macros() {
            meanings.insert(
                builtin.name,
                Meaning::Macro(MacroDef {
                    param_count: builtin.param_count,
                    optional_default: None,
                    body: Arc::clone(&builtin.body),
                    parameter_text: None,
                    protected: false,
                }),
            );
        }
        for &(name, selector) in select::Selector::BUILTINS {
            meanings.insert(name, Meaning::Selector(selector));
        }
        meanings
    })
}

#[derive(Clone, Debug)]
struct EnvDef {
    begin: MacroDef,
    end: Arc<[Token]>,
}

#[derive(Clone, Debug)]
struct ArrayContext {
    name: String,
    cell_scoped: bool,
    content_scope: usize,
}

#[derive(Clone, Debug)]
enum Meaning {
    Macro(MacroDef),
    Token(Token),
    Selector(select::Selector),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InputMode {
    #[default]
    Math,
    Text,
    DollarMath,
    ParenMath,
}

impl Meaning {
    fn work_size(&self) -> usize {
        match self {
            Self::Macro(def) => def
                .body
                .iter()
                .fold(0usize, |sum, token| sum.saturating_add(token.byte_len())),
            Self::Token(token) => token.byte_len(),
            Self::Selector(_) => 1,
        }
    }
}

struct WorkBudget {
    work_cap: usize,
    work: usize,
    failed: bool,
}

impl WorkBudget {
    fn charge(&mut self, amount: usize) -> ExpansionResult<()> {
        if self.failed {
            return Err(ExpansionFailure::Work {
                limit: self.work_cap,
            });
        }
        let next = self.work.saturating_add(amount);
        if next > self.work_cap {
            self.failed = true;
            return Err(ExpansionFailure::Work {
                limit: self.work_cap,
            });
        }
        self.work = next;
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
struct Scope {
    explicit_group: bool,
    input_mode: InputMode,
    macros: HashMap<String, Meaning>,
    environments: HashMap<String, Rc<EnvDef>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Edef,
}

pub(crate) fn expand(input: &str) -> ExpansionResult<String> {
    let cap = expansion_cap(input.len());
    Processor::new_with_limits(cap, expansion_work_cap(input.len())).expand_source(input)
}

pub(crate) struct Processor {
    scopes: Vec<Scope>,
    cap: usize,
    budget: Rc<RefCell<WorkBudget>>,
    produced: usize,
    environment_scopes: usize,
    parser_environments: Vec<ArrayContext>,
}

impl Processor {
    pub(crate) fn new_with_limits(cap: usize, work_cap: usize) -> Self {
        Self {
            scopes: vec![Scope::default()],
            cap,
            budget: Rc::new(RefCell::new(WorkBudget {
                work_cap,
                work: 0,
                failed: false,
            })),
            produced: 0,
            environment_scopes: 0,
            parser_environments: Vec::new(),
        }
    }

    fn expand_source(mut self, input: &str) -> ExpansionResult<String> {
        self.seed_builtins();
        let mut queue: VecDeque<Queued> = tokenize(input)
            .into_iter()
            .map(|token| Queued { token, depth: 0 })
            .collect();
        let mut output = Vec::new();
        let result = self.process_queue(&mut queue, &mut output, Mode::Normal);
        // A transactional read stopped by the work budget can also look like a
        // missing argument. Resource failures remain fatal in lenient mode.
        self.charge_work(0)?;
        result?;
        if self.scopes.iter().any(|scope| scope.explicit_group) {
            return Err(malformed("begingroup", SyntaxKind::GroupEnd));
        }
        render_tokens_bounded(&output, self.cap)
    }

    fn seed_builtins(&mut self) {
        // Builtin meanings live in an immutable process-wide table. Local and
        // global definitions in `scopes` shadow these entries without copying
        // every builtin name into each parser invocation.
    }

    fn push_scope(&mut self) -> ExpansionResult<()> {
        let limit = crate::error::DEFAULT_MAX_DEPTH;
        if self.scopes.len() > limit {
            return Err(ExpansionFailure::Depth { limit });
        }
        let input_mode = self.scopes.last().expect("global scope").input_mode;
        self.scopes.push(Scope {
            input_mode,
            ..Scope::default()
        });
        Ok(())
    }

    fn pop_scope(&mut self) -> ExpansionResult<()> {
        if self.scopes.last().is_some_and(|scope| scope.explicit_group) {
            return Err(malformed("begingroup", SyntaxKind::GroupEnd));
        }
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
        Ok(())
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
            self.scopes[0].environments.insert(name, Rc::new(env));
        } else {
            let scope = self.scopes.len() - 1;
            self.scopes[scope].environments.insert(name, Rc::new(env));
        }
    }

    fn has_macro(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .any(|scope| scope.macros.contains_key(name))
            || builtin_meanings().contains_key(name)
    }

    fn lookup_defined_meaning(&self, name: &str) -> Option<&Meaning> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.macros.get(name))
            .or_else(|| builtin_meanings().get(name))
    }

    fn single_definition_macro(&self, name: &str) -> Option<String> {
        let meaning = self.lookup_defined_meaning(name)?;
        let Meaning::Macro(def) = meaning else {
            return None;
        };
        if def.param_count == 0
            && def.body.len() == 1
            && let Token::ControlWord(effective) = &def.body[0]
            && matches!(effective.as_str(), "def" | "gdef" | "edef" | "xdef")
        {
            Some(effective.clone())
        } else {
            None
        }
    }

    fn lookup_macro_for_expansion(&mut self, name: &str) -> ExpansionResult<Option<Meaning>> {
        let Some(meaning) = self.lookup_defined_meaning(name).cloned() else {
            return Ok(None);
        };
        self.charge_work(meaning.work_size())?;
        Ok(Some(meaning))
    }

    fn lookup_environment(&self, name: &str) -> Option<Rc<EnvDef>> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.environments.get(name).cloned())
    }

    fn append(&mut self, out: &mut Vec<Token>, token: Token) -> ExpansionResult<()> {
        self.charge_work(token.byte_len().saturating_add(1))?;
        let next = self.produced.saturating_add(token.byte_len());
        if next > self.cap {
            return Err(ExpansionFailure::OutputBytes { limit: self.cap });
        }
        self.produced = next;
        out.push(token);
        Ok(())
    }

    fn charge_work(&mut self, amount: usize) -> ExpansionResult<()> {
        self.budget.borrow_mut().charge(amount)
    }

    fn prepend(
        &mut self,
        queue: &mut VecDeque<Queued>,
        tokens: Vec<Token>,
        depth: usize,
    ) -> ExpansionResult<()> {
        let bytes = tokens
            .iter()
            .fold(0usize, |sum, token| sum.saturating_add(token.byte_len()));
        self.charge_work(bytes.saturating_add(tokens.len()))?;
        for token in tokens.into_iter().rev() {
            queue.push_front(Queued { token, depth });
        }
        Ok(())
    }

    fn process_queue(
        &mut self,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
        mode: Mode,
    ) -> ExpansionResult<()> {
        while let Some(Queued { token, depth }) = queue.pop_front() {
            self.charge_work(token.byte_len().saturating_add(1))?;
            match token {
                Token::BeginGroup => {
                    self.append(out, Token::BeginGroup)?;
                    self.push_scope()?;
                },
                Token::EndGroup => {
                    self.append(out, Token::EndGroup)?;
                    self.pop_scope()?;
                },
                Token::Verbatim(text) => {
                    self.append(out, Token::Verbatim(text))?;
                },
                Token::EndEnvironment => {
                    self.pop_scope()?;
                    self.environment_scopes = self.environment_scopes.saturating_sub(1);
                },
                Token::ControlWord(name) => {
                    if name == "expandafter" {
                        self.expandafter(queue, mode, depth)?;
                        continue;
                    }
                    if mode == Mode::Edef {
                        if name == "noexpand" {
                            self.edef_noexpand(queue, out)?;
                        } else if !self.expand_macro_from_queue(
                            &name,
                            depth,
                            queue,
                            out,
                            Mode::Edef,
                        )? {
                            self.append(out, Token::ControlWord(name))?;
                        }
                        continue;
                    }

                    if self.definition_command(&name, queue)? {
                        continue;
                    }
                    if self.environment_command(&name, depth, queue, out)? {
                        continue;
                    }
                    if name == "noexpand" {
                        self.noexpand(queue, out, depth)?;
                    } else if !self.expand_macro_from_queue(
                        &name,
                        depth,
                        queue,
                        out,
                        Mode::Normal,
                    )? {
                        self.emit_native(Token::ControlWord(name), queue, out, mode, depth)?;
                    }
                },
                Token::ControlSymbol(symbol) => {
                    let name = control_key_symbol(symbol);
                    let expanded = self.expand_macro_from_queue(&name, depth, queue, out, mode)?;
                    if !expanded {
                        self.emit_native(Token::ControlSymbol(symbol), queue, out, mode, depth)?;
                    }
                },
                Token::Char(ch) => self.emit_native(Token::Char(ch), queue, out, mode, depth)?,
                Token::NoExpand(_) => {
                    // `\noexpand` makes an expandable control sequence act
                    // like `\relax`; the token is intentionally discarded.
                },
                Token::Frozen(token) => {
                    self.emit_native(*token, queue, out, mode, depth)?;
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
    ) -> ExpansionResult<bool> {
        let Some(meaning) = self.lookup_macro_for_expansion(name)? else {
            return Ok(false);
        };
        if self.expand_replacement(name, &meaning, queue, mode, depth)? {
            return Ok(true);
        }
        match (mode, meaning) {
            (Mode::Normal, Meaning::Token(token)) => {
                self.emit_native(token, queue, out, mode, depth)?;
            },
            // A protected macro or a let copy of an unexpandable token retains
            // its identity in an expanded definition.
            _ => self.append(out, control_sequence_token(name))?,
        }
        Ok(true)
    }

    /// Put one expandable meaning on the front of the input. Both normal
    /// expansion and expandafter must consume exactly the same arguments.
    fn expand_replacement(
        &mut self,
        name: &str,
        meaning: &Meaning,
        queue: &mut VecDeque<Queued>,
        mode: Mode,
        depth: usize,
    ) -> ExpansionResult<bool> {
        if matches!(meaning, Meaning::Token(_))
            || matches!(meaning, Meaning::Macro(def) if mode == Mode::Edef && def.protected)
        {
            return Ok(false);
        }
        if depth >= MAX_EXPANSION_DEPTH {
            return Err(ExpansionFailure::Depth {
                limit: MAX_EXPANSION_DEPTH,
            });
        }
        let missing_argument = || ExpansionFailure::Arguments {
            command: name.trim_start_matches('\0').into(),
        };
        let replacement = match meaning {
            Meaning::Selector(selector) => selector
                .select(
                    queue,
                    self.budget.clone(),
                    self.scopes.last().expect("global scope").input_mode == InputMode::Text,
                )
                .ok_or_else(missing_argument)?,
            Meaning::Macro(def) => {
                let args =
                    consume_args(queue, def, self.budget.clone()).ok_or_else(missing_argument)?;
                substitute(def, &args, self.cap)?
            },
            Meaning::Token(_) => unreachable!("unexpandable meaning checked above"),
        };
        self.prepend(queue, replacement, depth + 1)?;
        Ok(true)
    }

    fn definition_command(
        &mut self,
        name: &str,
        queue: &mut VecDeque<Queued>,
    ) -> ExpansionResult<bool> {
        if matches!(name, "def" | "gdef" | "edef" | "xdef") {
            let mut txn = QueueTxn::new(queue, self.budget.clone());
            let result = self.parse_def_txn(
                name,
                false,
                matches!(name, "edef" | "xdef"),
                false,
                &mut txn,
            )?;
            return Ok(if result {
                txn.commit();
                true
            } else {
                txn.rollback();
                false
            });
        }
        if name == "let" {
            let mut txn = QueueTxn::new(queue, self.budget.clone());
            let result = self.parse_let_txn(false, &mut txn)?;
            return Ok(if result {
                txn.commit();
                true
            } else {
                txn.rollback();
                false
            });
        }
        if name == "futurelet" {
            return self.parse_futurelet(queue, false);
        }
        if matches!(name, "newcommand" | "renewcommand" | "providecommand") {
            let mut txn = QueueTxn::new(queue, self.budget.clone());
            let result = self.parse_newcommand_txn(name, &mut txn)?;
            return Ok(if result {
                txn.commit();
                true
            } else {
                txn.rollback();
                false
            });
        }
        if name == "newenvironment" {
            let mut txn = QueueTxn::new(queue, self.budget.clone());
            let result = self.parse_newenvironment_txn(&mut txn)?;
            return Ok(if result {
                txn.commit();
                true
            } else {
                txn.rollback();
                false
            });
        }
        if !matches!(name, "global" | "long" | "protected") {
            return Ok(false);
        }

        // Prefix parsing is transactional.  If the next command is not a
        // definition primitive, restoring it leaves `\global` in the output
        // for the strict parser to reject.
        let mut txn = QueueTxn::new(queue, self.budget.clone());
        let mut global = name == "global";
        let mut protected = name == "protected";
        loop {
            let Some(next_name) = txn.peek_control_word() else {
                txn.rollback();
                return Err(malformed(name, SyntaxKind::PrefixAssignment));
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
            if next_name == "protected" {
                protected = true;
                txn.pop();
                continue;
            }
            break;
        }
        let Some(command) = txn.peek_control_word() else {
            txn.rollback();
            return Err(malformed(name, SyntaxKind::PrefixAssignment));
        };
        if matches!(command.as_str(), "def" | "gdef" | "edef" | "xdef") {
            txn.pop();
            let result = self.parse_def_txn(
                &command,
                global || command == "gdef" || command == "xdef",
                matches!(command.as_str(), "edef" | "xdef"),
                protected,
                &mut txn,
            )?;
            return Ok(if result {
                txn.commit();
                true
            } else {
                txn.rollback();
                false
            });
        }
        if protected && matches!(command.as_str(), "let" | "futurelet") {
            return Err(malformed("protected", SyntaxKind::ProtectedAssignment));
        }
        if command == "let" {
            txn.pop();
            let result = self.parse_let_txn(global, &mut txn)?;
            return Ok(if result {
                txn.commit();
                true
            } else {
                txn.rollback();
                false
            });
        }
        if command == "futurelet" {
            txn.pop();
            let result = self.parse_futurelet_txn(global, &mut txn)?;
            return Ok(if result {
                txn.commit();
                true
            } else {
                txn.rollback();
                false
            });
        }
        // TeX permits a macro expanding to `\def` after a prefix.
        if let Some(effective) = self.single_definition_macro(&command) {
            txn.pop();
            let result = self.parse_def_txn(
                &effective,
                global || effective == "gdef" || effective == "xdef",
                matches!(effective.as_str(), "edef" | "xdef"),
                protected,
                &mut txn,
            )?;
            return Ok(if result {
                txn.commit();
                true
            } else {
                txn.rollback();
                false
            });
        }
        txn.rollback();
        Err(malformed(name, SyntaxKind::PrefixAssignment))
    }

    fn parse_def_txn(
        &mut self,
        command: &str,
        global: bool,
        expand_body: bool,
        protected: bool,
        txn: &mut QueueTxn<'_>,
    ) -> ExpansionResult<bool> {
        let target = txn
            .pop_control_sequence()
            .ok_or_else(|| malformed(command, SyntaxKind::ControlSequenceTarget))?;
        txn.skip_whitespace();
        let mut parameter_tokens = Vec::new();
        loop {
            let Some(token) = txn.peek_token() else {
                return Err(malformed(command, SyntaxKind::ReplacementBody));
            };
            if matches!(token, Token::BeginGroup) {
                break;
            }
            parameter_tokens.push(
                txn.pop_token()
                    .ok_or_else(|| malformed(command, SyntaxKind::ReplacementBody))?,
            );
        }
        let body = txn
            .pop_group()
            .ok_or_else(|| malformed(command, SyntaxKind::ReplacementBody))?;
        let parameter_text = parse_parameter_text(&parameter_tokens)
            .ok_or_else(|| malformed(command, SyntaxKind::ParameterText))?;
        let count = parameter_text.parameter_count();
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
                body: body.into(),
                parameter_text: Some(parameter_text),
                protected,
            }),
            global || command == "gdef" || command == "xdef",
        );
        Ok(true)
    }

    fn parse_let_txn(&mut self, global: bool, txn: &mut QueueTxn<'_>) -> ExpansionResult<bool> {
        let target = txn
            .pop_control_sequence()
            .ok_or_else(|| malformed("let", SyntaxKind::ControlSequenceTarget))?;
        txn.skip_whitespace();
        if matches!(txn.peek_token(), Some(Token::Char('='))) {
            txn.pop();
            // Exactly one space after an equals sign is optional syntax; a
            // second space is the source token being assigned (for example
            // the `\let\space= ` idiom).
            if matches!(txn.peek_token(), Some(Token::Char(' '))) {
                txn.pop();
            }
        }
        let source = txn
            .pop_token()
            .ok_or_else(|| malformed("let", SyntaxKind::SourceToken))?;
        if matches!(source, Token::EndGroup) {
            return Err(malformed("let", SyntaxKind::SourceToken));
        }
        let meaning = match &source {
            Token::ControlWord(name) => match self.lookup_macro_for_expansion(name)? {
                Some(meaning) => meaning,
                None => Meaning::Token(source.clone()),
            },
            Token::ControlSymbol(symbol) => {
                match self.lookup_macro_for_expansion(&control_key_symbol(*symbol))? {
                    Some(meaning) => meaning,
                    None => Meaning::Token(Token::ControlSymbol(*symbol)),
                }
            },
            _ => Meaning::Token(source),
        };
        self.define_macro(target, meaning, global);
        Ok(true)
    }

    fn parse_futurelet(
        &mut self,
        queue: &mut VecDeque<Queued>,
        global: bool,
    ) -> ExpansionResult<bool> {
        let mut txn = QueueTxn::new(queue, self.budget.clone());
        let result = self.parse_futurelet_txn(global, &mut txn)?;
        if result {
            txn.commit();
        } else {
            txn.rollback();
        }
        Ok(result)
    }

    /// `\futurelet\name<middle><next>` assigns the meaning of `<next>` to
    /// `name`, then puts `<next>` and `<middle>` back in front of the stream so
    /// both execute in their original order.  This raw-token lookahead is
    /// deliberately not macro expansion: the assigned meaning is a snapshot
    /// just like `\let`.
    fn parse_futurelet_txn(
        &mut self,
        global: bool,
        txn: &mut QueueTxn<'_>,
    ) -> ExpansionResult<bool> {
        let target = txn
            .pop_control_sequence()
            .ok_or_else(|| malformed("futurelet", SyntaxKind::ControlSequenceTarget))?;
        let middle = txn
            .pop()
            .ok_or_else(|| malformed("futurelet", SyntaxKind::MiddleToken))?;
        let next = txn
            .pop()
            .ok_or_else(|| malformed("futurelet", SyntaxKind::LookaheadToken))?;
        let source = next.token.clone();
        let meaning = match &source {
            Token::ControlWord(name) => match self.lookup_macro_for_expansion(name)? {
                Some(meaning) => meaning,
                None => Meaning::Token(source.clone()),
            },
            Token::ControlSymbol(symbol) => {
                match self.lookup_macro_for_expansion(&control_key_symbol(*symbol))? {
                    Some(meaning) => meaning,
                    None => Meaning::Token(Token::ControlSymbol(*symbol)),
                }
            },
            _ => Meaning::Token(source),
        };
        self.define_macro(target, meaning, global);
        // QueueTxn's consumed tokens would otherwise be restored on drop.  We
        // commit the assignment and then explicitly put the two lookahead
        // tokens back; they are part of the command's semantics, not consumed
        // input.
        txn.commit();
        self.charge_work(
            next.token
                .byte_len()
                .saturating_add(middle.token.byte_len())
                .saturating_add(2),
        )?;
        txn.queue.push_front(next);
        txn.queue.push_front(middle);
        Ok(true)
    }

    fn parse_newcommand_txn(
        &mut self,
        command: &str,
        txn: &mut QueueTxn<'_>,
    ) -> ExpansionResult<bool> {
        txn.skip_whitespace();
        if matches!(txn.peek_token(), Some(Token::Char('*'))) {
            txn.pop();
            txn.skip_whitespace();
        }
        let target = txn.pop_newcommand_target()?;
        let (count, default) = declaration_args(command, txn)?;
        let body = txn
            .pop_group()
            .ok_or_else(|| malformed(command, SyntaxKind::ReplacementBody))?;
        if command != "providecommand" || !self.has_macro(&target) {
            self.define_macro(
                target,
                Meaning::Macro(MacroDef {
                    param_count: count,
                    optional_default: default,
                    body: body.into(),
                    parameter_text: None,
                    protected: false,
                }),
                false,
            );
        }
        Ok(true)
    }

    fn parse_newenvironment_txn(&mut self, txn: &mut QueueTxn<'_>) -> ExpansionResult<bool> {
        txn.skip_whitespace();
        if matches!(txn.peek_token(), Some(Token::Char('*'))) {
            txn.pop();
            txn.skip_whitespace();
        }
        let name = txn
            .pop_group()
            .ok_or_else(|| malformed("newenvironment", SyntaxKind::EnvironmentName))?;
        let env_name = token_text(&name);
        if env_name.trim().is_empty() {
            return Err(malformed("newenvironment", SyntaxKind::EnvironmentName));
        }
        let (param_count, optional_default) = declaration_args("newenvironment", txn)?;
        let begin = txn
            .pop_group()
            .ok_or_else(|| malformed("newenvironment", SyntaxKind::EnvironmentBegin))?;
        let end = txn
            .pop_group()
            .ok_or_else(|| malformed("newenvironment", SyntaxKind::EnvironmentEnd))?;
        self.define_environment(
            env_name.trim().to_owned(),
            EnvDef {
                begin: MacroDef {
                    param_count,
                    optional_default,
                    body: begin.into(),
                    parameter_text: None,
                    protected: false,
                },
                end: end.into(),
            },
            false,
        );
        Ok(true)
    }

    fn environment_command(
        &mut self,
        name: &str,
        depth: usize,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
    ) -> ExpansionResult<bool> {
        if name != "begin" && name != "end" {
            return Ok(false);
        }
        let mut txn = QueueTxn::new(queue, self.budget.clone());
        txn.skip_whitespace();
        let Some(group) = txn.pop_group() else {
            txn.rollback();
            return Ok(false);
        };
        // LaTeX expands the environment name when looking up its definition.
        // Keep literal names on the cheap path; aliases use the same bounded,
        // non-executing expansion used for the body of an expanded definition.
        let group = if group.iter().any(|token| !matches!(token, Token::Char(_))) {
            self.expand_edef_body(group)?
        } else {
            group
        };
        let env_name = token_text(&group).trim().to_owned();
        if let Some(env) = self.lookup_environment(&env_name) {
            txn.commit();
            drop(txn);
            self.expand_defined_environment(name, &env, depth, queue)?;
            return Ok(true);
        }
        // Parser-owned arrays open one scope per cell in addition to the
        // environment group, so a definition cannot leak across a separator.
        let fences = packages::smallmatrix_fences(&env_name);
        let Some(cell_scoped) = fences
            .map(|_| true)
            .or_else(|| parser_environment_kind(&env_name))
        else {
            txn.rollback();
            return Ok(false);
        };
        txn.commit();
        drop(txn);

        if let Some((open, close)) = fences.filter(|_| {
            name == "begin"
                || self
                    .parser_environments
                    .last()
                    .is_some_and(|env| env.name == env_name)
        }) {
            // mathtools gives these fences independent opening and closing
            // atom classes. A bare left/right pair adds space beside a variable.
            let inner = if env_name.ends_with('*') {
                "smallmatrix*"
            } else {
                "smallmatrix"
            };
            let expansion = if name == "begin" {
                format!(r"\mathopen{{}}\mathclose{{\left{open}\begin{{{inner}}}")
            } else {
                format!(r"\end{{{inner}}}\right{close}}}")
            };
            for token in tokenize(&expansion) {
                self.append(out, token)?;
            }
        } else {
            self.append(out, Token::ControlWord(name.to_owned()))?;
            self.append(out, Token::BeginGroup)?;
            for token in group {
                self.append(out, token)?;
            }
            self.append(out, Token::EndGroup)?;
        }
        if name == "begin" {
            self.push_scope()?;
            if cell_scoped {
                self.push_scope()?;
            }
            self.parser_environments.push(ArrayContext {
                name: env_name,
                cell_scoped,
                content_scope: self.scopes.len(),
            });
        } else if self
            .parser_environments
            .last()
            .is_some_and(|env| env.name == env_name)
        {
            if cell_scoped {
                self.pop_scope()?;
            }
            self.pop_scope()?;
            self.parser_environments.pop();
        }
        Ok(true)
    }

    fn expand_defined_environment(
        &mut self,
        name: &str,
        env: &EnvDef,
        depth: usize,
        queue: &mut VecDeque<Queued>,
    ) -> ExpansionResult<()> {
        if name == "begin" {
            let args = consume_args(queue, &env.begin, self.budget.clone())
                .ok_or_else(|| malformed("begin", SyntaxKind::MissingArguments))?;
            let replacement = substitute(&env.begin, &args, self.cap)?;
            self.push_scope()?;
            self.environment_scopes += 1;
            self.prepend(queue, replacement, depth + 1)?;
        } else {
            if self.environment_scopes > 0 {
                // The end body must run before the environment scope is
                // restored; place a private marker after it.
                self.charge_work(1)?;
                queue.push_front(Queued {
                    token: Token::EndEnvironment,
                    depth,
                });
            }
            self.prepend(queue, env.end.to_vec(), depth + 1)?;
        }
        Ok(())
    }

    fn noexpand(
        &mut self,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
        depth: usize,
    ) -> ExpansionResult<()> {
        let mut txn = QueueTxn::new(queue, self.budget.clone());
        txn.skip_whitespace();
        let Some(token) = txn.pop_token() else {
            txn.rollback();
            self.append(out, Token::ControlWord("noexpand".to_owned()))?;
            return Ok(());
        };
        txn.commit();
        drop(txn);
        if let Some(replacement) = self.noexpand_replacement(token)? {
            match replacement {
                Token::NoExpand(_) => {
                    queue.push_front(Queued {
                        token: replacement,
                        depth,
                    });
                },
                Token::Frozen(inner) => self.append(out, *inner)?,
                token @ (Token::ControlWord(_) | Token::ControlSymbol(_)) => {
                    queue.push_front(Queued { token, depth });
                },
                token => self.append(out, token)?,
            }
        }
        Ok(())
    }

    /// Apply TeX's `\expandafter`: retain the first token, expand only the
    /// immediately following expandable token once, then continue with the
    /// retained token in front of that replacement.
    fn expandafter(
        &mut self,
        queue: &mut VecDeque<Queued>,
        mode: Mode,
        depth: usize,
    ) -> ExpansionResult<()> {
        let Some(first) = queue.pop_front() else {
            return Ok(());
        };
        self.charge_work(first.token.byte_len().saturating_add(1))?;
        let _ = self.expand_one_after(queue, mode, depth)?;
        queue.push_front(first);
        Ok(())
    }

    fn expand_one_after(
        &mut self,
        queue: &mut VecDeque<Queued>,
        mode: Mode,
        depth: usize,
    ) -> ExpansionResult<bool> {
        let Some(item) = queue.pop_front() else {
            return Ok(false);
        };
        self.charge_work(item.token.byte_len().saturating_add(1))?;
        let item_depth = item.depth.max(depth);
        if item_depth >= MAX_EXPANSION_DEPTH {
            return Err(ExpansionFailure::Depth {
                limit: MAX_EXPANSION_DEPTH,
            });
        }
        match item.token {
            Token::ControlWord(name) if name == "expandafter" => {
                self.expandafter(queue, mode, item_depth + 1)?;
                Ok(true)
            },
            Token::ControlWord(name) if name == "noexpand" => {
                let mut txn = QueueTxn::new(queue, self.budget.clone());
                txn.skip_whitespace();
                let Some(token) = txn.pop() else {
                    txn.rollback();
                    drop(txn);
                    queue.push_front(Queued {
                        token: Token::ControlWord(name),
                        depth: item.depth,
                    });
                    return Ok(false);
                };
                txn.commit();
                drop(txn);
                let Some(replacement) = self.noexpand_replacement(token.token)? else {
                    return Ok(false);
                };
                queue.push_front(Queued {
                    token: replacement,
                    depth: item_depth + 1,
                });
                Ok(true)
            },
            Token::ControlWord(name) => self.expand_one_named(&name, item_depth, queue, mode),
            Token::ControlSymbol(symbol) => {
                let name = control_key_symbol(symbol);
                self.expand_one_named(&name, item_depth, queue, mode)
            },
            token => {
                queue.push_front(Queued {
                    token,
                    depth: item.depth,
                });
                Ok(false)
            },
        }
    }

    fn expand_one_named(
        &mut self,
        name: &str,
        depth: usize,
        queue: &mut VecDeque<Queued>,
        mode: Mode,
    ) -> ExpansionResult<bool> {
        if let Some(meaning) = self.lookup_macro_for_expansion(name)?
            && self.expand_replacement(name, &meaning, queue, mode, depth)?
        {
            return Ok(true);
        }
        queue.push_front(Queued {
            token: control_sequence_token(name),
            depth,
        });
        Ok(false)
    }

    /// Return the ordinary `\noexpand` result. `None` means an empty input;
    /// expandable control sequences become a relax-like marker, while
    /// unexpandable meanings remain executable.
    fn noexpand_replacement(&mut self, token: Token) -> ExpansionResult<Option<Token>> {
        let Token::ControlWord(name) = &token else {
            let Token::ControlSymbol(symbol) = &token else {
                return Ok(Some(token));
            };
            let Some(meaning) = self.lookup_macro_for_expansion(&control_key_symbol(*symbol))?
            else {
                return Ok(Some(token));
            };
            return Ok(match meaning {
                Meaning::Macro(_) | Meaning::Selector(_) => Some(Token::NoExpand(Box::new(token))),
                Meaning::Token(meaning) => Some(Token::Frozen(Box::new(meaning))),
            });
        };
        let Some(meaning) = self.lookup_macro_for_expansion(name)? else {
            return Ok(Some(token));
        };
        Ok(match meaning {
            Meaning::Macro(_) | Meaning::Selector(_) => Some(Token::NoExpand(Box::new(token))),
            Meaning::Token(meaning) => Some(Token::Frozen(Box::new(meaning))),
        })
    }

    fn edef_noexpand(
        &mut self,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
    ) -> ExpansionResult<()> {
        let mut txn = QueueTxn::new(queue, self.budget.clone());
        txn.skip_whitespace();
        let Some(token) = txn.pop_token() else {
            txn.rollback();
            self.append(out, Token::ControlWord("noexpand".to_owned()))?;
            return Ok(());
        };
        txn.commit();
        self.append(out, token)
    }

    /// Native commands remain in the output for the AST parser. Track only
    /// the mode transitions that affect the meaning of an expandable selector.
    fn emit_native(
        &mut self,
        token: Token,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
        mode: Mode,
        depth: usize,
    ) -> ExpansionResult<()> {
        if mode == Mode::Normal {
            if let Token::ControlWord(name) = &token {
                match name.as_str() {
                    "begingroup" => {
                        self.push_scope()?;
                        self.scopes.last_mut().expect("new group").explicit_group = true;
                        return Ok(());
                    },
                    "endgroup" => {
                        let current = self.scopes.last_mut().expect("global scope");
                        if !current.explicit_group {
                            return Err(malformed("endgroup", SyntaxKind::GroupEnd));
                        }
                        current.explicit_group = false;
                        return self.pop_scope();
                    },
                    _ => {},
                }

                let argument_mode = if text_style(name).is_some()
                    || matches!(name.as_str(), "angl" | "reflectbox")
                {
                    Some(InputMode::Text)
                } else if name == "mathreflectbox" {
                    Some(InputMode::Math)
                } else {
                    None
                };
                if let Some(input_mode) = argument_mode
                    && self.open_mode_argument(name, input_mode, depth, queue, out)?
                {
                    return Ok(());
                }
            }
            // Only an actual separator in the current alignment ends a cell.
            // A separator inside a nested group belongs to the inner construct.
            // A surviving space after a control word came from another
            // token list, e.g. #1 followed by a space in a replacement. In a
            // text argument it needs an explicit control space when the
            // tokens are serialized, or the text lexer will swallow it.
            if self.scopes.last().expect("global scope").input_mode == InputMode::Text
                && matches!(token, Token::Char(' ' | '\t'))
                && out
                    .last()
                    .is_some_and(|t| matches!(t, Token::ControlWord(_)))
            {
                return self.append(out, Token::ControlSymbol(' '));
            }
            let cell_separator = matches!(token, Token::Char('&') | Token::ControlSymbol('\\'))
                || matches!(&token, Token::ControlWord(name) if name == "cr");
            if cell_separator
                && self
                    .parser_environments
                    .last()
                    .is_some_and(|env| env.cell_scoped && env.content_scope == self.scopes.len())
            {
                self.pop_scope()?;
                self.push_scope()?;
            }
            let input_mode = &mut self.scopes.last_mut().expect("global scope").input_mode;
            *input_mode = match (*input_mode, &token) {
                (InputMode::Text, Token::Char('$')) => InputMode::DollarMath,
                (InputMode::DollarMath, Token::Char('$')) => InputMode::Text,
                (InputMode::Text, Token::ControlSymbol('(')) => InputMode::ParenMath,
                (InputMode::ParenMath, Token::ControlSymbol(')')) => InputMode::Text,
                (current, _) => current,
            };
        }
        self.append(out, token)
    }

    fn open_mode_argument(
        &mut self,
        name: &str,
        input_mode: InputMode,
        depth: usize,
        queue: &mut VecDeque<Queued>,
        out: &mut Vec<Token>,
    ) -> ExpansionResult<bool> {
        let mut argument = None;
        for item in queue.iter() {
            self.charge_work(item.token.byte_len().saturating_add(1))?;
            if !item.token.is_whitespace() {
                argument = Some(item);
                break;
            }
        }
        let Some(argument) = argument else {
            return Ok(false);
        };
        // Braced contents expand in the argument's mode through their closing
        // brace. A bare macro is one argument even if its replacement is longer.
        let grouped = match &argument.token {
            Token::BeginGroup => true,
            Token::ControlWord(word) if self.has_macro(word) => false,
            _ => return Ok(false),
        };
        while queue.front().is_some_and(|q| q.token.is_whitespace()) {
            queue.pop_front();
        }
        let first = queue.pop_front().expect("argument was present");
        self.append(out, Token::ControlWord(name.to_owned()))?;
        self.append(out, Token::BeginGroup)?;
        self.push_scope()?;
        self.scopes.last_mut().expect("argument scope").input_mode = input_mode;
        if !grouped {
            self.charge_work(1)?;
            queue.push_front(Queued {
                token: Token::EndGroup,
                depth,
            });
            queue.push_front(first);
        }
        Ok(true)
    }

    fn expand_edef_body(&mut self, body: Vec<Token>) -> ExpansionResult<Vec<Token>> {
        let body_bytes = body
            .iter()
            .fold(0usize, |sum, token| sum.saturating_add(token.byte_len()));
        self.charge_work(body_bytes.saturating_add(body.len()))?;
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
    budget: Rc<RefCell<WorkBudget>>,
    committed: bool,
}

impl<'a> QueueTxn<'a> {
    fn new(queue: &'a mut VecDeque<Queued>, budget: Rc<RefCell<WorkBudget>>) -> Self {
        Self {
            queue,
            taken: Vec::new(),
            budget,
            committed: false,
        }
    }

    fn pop(&mut self) -> Option<Queued> {
        let cost = self.queue.front()?.token.byte_len().saturating_add(1);
        self.budget.borrow_mut().charge(cost).ok()?;
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

    fn peek_token_at(&mut self, offset: usize) -> Option<&Token> {
        let cost = self.queue.get(offset)?.token.byte_len().saturating_add(1);
        self.budget.borrow_mut().charge(cost).ok()?;
        self.queue.get(offset).map(|item| &item.token)
    }

    fn peek_control_word(&mut self) -> Option<String> {
        let name_len = match self.peek_token()? {
            Token::ControlWord(name) => name.len().saturating_add(1),
            _ => return None,
        };
        self.budget.borrow_mut().charge(name_len).ok()?;
        match self.peek_token()? {
            Token::ControlWord(name) => Some(name.clone()),
            _ => None,
        }
    }

    fn pop_control_sequence(&mut self) -> Option<String> {
        match self.pop_token()? {
            Token::ControlWord(name) => Some(name),
            Token::ControlSymbol(symbol) => Some(control_key_symbol(symbol)),
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

    fn pop_newcommand_target(&mut self) -> ExpansionResult<String> {
        self.skip_whitespace();
        if !matches!(self.peek_token(), Some(Token::BeginGroup)) {
            return self
                .pop_control_sequence()
                .ok_or_else(|| malformed("newcommand", SyntaxKind::ControlSequenceTarget));
        }
        let inner = self
            .pop_group()
            .ok_or_else(|| malformed("newcommand", SyntaxKind::UnclosedTarget))?;
        let mut index = 0;
        while inner.get(index).is_some_and(Token::is_whitespace) {
            index += 1;
        }
        let target = match inner.get(index) {
            Some(Token::ControlWord(name)) => name.clone(),
            Some(Token::ControlSymbol(symbol)) => control_key_symbol(*symbol),
            _ => {
                return Err(malformed("newcommand", SyntaxKind::TargetGroup));
            },
        };
        index += 1;
        while inner.get(index).is_some_and(Token::is_whitespace) {
            index += 1;
        }
        if index != inner.len() {
            return Err(malformed("newcommand", SyntaxKind::TargetGroup));
        }
        Ok(target)
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
        let rollback_cost = self.taken.iter().fold(0usize, |sum, item| {
            sum.saturating_add(item.token.byte_len().saturating_add(1))
        });
        let _ = self.budget.borrow_mut().charge(rollback_cost);
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

/// Let the dedicated argument module validate and represent the already
/// tokenized parameter text.  Converting each header token to its exact token
/// spelling keeps delimiter identity without detokenizing and retokenizing a
/// synthetic definition string.
fn parse_parameter_text(tokens: &[Token]) -> Option<ParameterText> {
    let parameter_tokens = tokens
        .iter()
        .map(parameter_token)
        .collect::<Option<Vec<_>>>()?;
    macro_args::parse_parameter_text(&parameter_tokens).ok()
}

fn parameter_token(token: &Token) -> Option<macro_args::ParameterToken> {
    let text = match token {
        Token::Char(ch) => ch.to_string(),
        Token::BeginGroup => "{".to_owned(),
        Token::EndGroup => "}".to_owned(),
        Token::ControlWord(name) => format!("\\{name}"),
        Token::ControlSymbol(symbol) => format!("\\{symbol}"),
        Token::Verbatim(text) => text.clone(),
        Token::EndEnvironment => return None,
        Token::NoExpand(inner) => return parameter_token(inner),
        Token::Frozen(inner) => return parameter_token(inner),
    };
    Some(macro_args::ParameterToken::from_text(text))
}

/// Read the LaTeX declaration's optional argument count and optional first default.
fn declaration_args(
    command: &str,
    txn: &mut QueueTxn<'_>,
) -> ExpansionResult<(u8, Option<Vec<Token>>)> {
    txn.skip_whitespace();
    let mut count = 0u8;
    if let Some(number) = txn.pop_bracket() {
        let text = token_text(&number);
        let parsed = text
            .trim()
            .parse::<u8>()
            .ok()
            .filter(|_| text.trim().bytes().all(|b| b.is_ascii_digit()))
            .ok_or_else(|| malformed(command, SyntaxKind::ParameterCount))?;
        count = parsed;
        txn.skip_whitespace();
    } else if txn.peek_token_is('[') {
        return Err(malformed(command, SyntaxKind::UnclosedParameterCount));
    }
    let default = if count > 0 {
        if txn.peek_token_is('[') {
            let default = txn
                .pop_bracket()
                .ok_or_else(|| malformed(command, SyntaxKind::OptionalDefault))?;
            txn.skip_whitespace();
            Some(default)
        } else {
            None
        }
    } else {
        if txn.peek_token_is('[') {
            return Err(malformed(
                command,
                SyntaxKind::OptionalDefaultWithoutParameter,
            ));
        }
        None
    };
    Ok((count, default))
}

fn consume_args(
    queue: &mut VecDeque<Queued>,
    def: &MacroDef,
    budget: Rc<RefCell<WorkBudget>>,
) -> Option<Vec<Vec<Token>>> {
    if let Some(parameter_text) = &def.parameter_text {
        return consume_parameter_args(queue, parameter_text, budget);
    }
    let mut txn = QueueTxn::new(queue, budget);
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

fn consume_parameter_args(
    queue: &mut VecDeque<Queued>,
    parameter_text: &ParameterText,
    budget: Rc<RefCell<WorkBudget>>,
) -> Option<Vec<Vec<Token>>> {
    let mut txn = QueueTxn::new(queue, budget);
    let delimiters = parameter_text.delimiters();
    if !consume_exact_delimiter(&mut txn, &delimiters[0]) {
        txn.rollback();
        return None;
    }
    let mut args = Vec::with_capacity(parameter_text.parameter_count() as usize);
    for index in 0..parameter_text.parameter_count() as usize {
        let delimiter = &delimiters[index + 1];
        let argument = if delimiter.is_empty() {
            txn.skip_whitespace();
            if matches!(txn.peek_token(), Some(Token::BeginGroup)) {
                txn.pop_group()?
            } else {
                let token = txn.pop_token()?;
                if matches!(token, Token::EndGroup) {
                    return None;
                }
                vec![token]
            }
        } else {
            let mut depth = 0usize;
            let mut argument = Vec::new();
            loop {
                if depth == 0 && delimiter_matches(&mut txn, delimiter) {
                    let _ = consume_exact_delimiter(&mut txn, delimiter);
                    break argument;
                }
                let token = txn.pop_token()?;
                match token {
                    Token::BeginGroup => {
                        depth += 1;
                        argument.push(Token::BeginGroup);
                    },
                    Token::EndGroup => {
                        if depth == 0 {
                            return None;
                        }
                        depth -= 1;
                        argument.push(Token::EndGroup);
                    },
                    other => argument.push(other),
                }
            }
        };
        args.push(argument);
    }
    txn.commit();
    Some(args)
}

fn consume_exact_delimiter(
    txn: &mut QueueTxn<'_>,
    delimiter: &[macro_args::ParameterToken],
) -> bool {
    for expected in delimiter {
        let Some(token) = txn.peek_token() else {
            return false;
        };
        if !token_matches_parameter(token, expected.as_str()) {
            return false;
        }
        txn.pop();
    }
    true
}

fn delimiter_matches(txn: &mut QueueTxn<'_>, delimiter: &[macro_args::ParameterToken]) -> bool {
    delimiter.iter().enumerate().all(|(offset, expected)| {
        txn.peek_token_at(offset)
            .is_some_and(|token| token_matches_parameter(token, expected.as_str()))
    })
}

fn token_matches_parameter(token: &Token, expected: &str) -> bool {
    match token {
        Token::Char(c) => {
            expected.chars().eq(std::iter::once(*c)) && expected.len() == c.len_utf8()
        },
        Token::BeginGroup => expected == "{",
        Token::EndGroup => expected == "}",
        Token::ControlWord(name) => expected.strip_prefix('\\') == Some(name.as_str()),
        Token::ControlSymbol(c) => {
            let mut chars = expected.chars();
            chars.next() == Some('\\') && chars.next() == Some(*c) && chars.next().is_none()
        },
        Token::Verbatim(text) => expected == text,
        Token::EndEnvironment => false,
        Token::NoExpand(inner) => token_matches_parameter(inner, expected),
        Token::Frozen(inner) => token_matches_parameter(inner, expected),
    }
}

fn control_sequence_token(key: &str) -> Token {
    if let Some(symbol) = key.strip_prefix('\0') {
        Token::ControlSymbol(symbol.chars().next().expect("control symbol key"))
    } else {
        Token::ControlWord(key.to_owned())
    }
}

fn control_key_symbol(symbol: char) -> String {
    // Word names are stored without a leading backslash.  A nonprinting
    // prefix keeps control-symbol meanings in the same map without colliding
    // with a word such as `space`.
    let mut key = String::with_capacity(2);
    key.push('\u{0}');
    key.push(symbol);
    key
}

fn parser_environment_kind(name: &str) -> Option<bool> {
    // The first group is the environment itself; matrix/array-family bodies
    // are split into independently scoped cells by `&` and `\\`.
    match name {
        "matrix" | "pmatrix" | "bmatrix" | "Bmatrix" | "vmatrix" | "Vmatrix" | "smallmatrix"
        | "smallmatrix*" | "matrix*" | "pmatrix*" | "bmatrix*" | "Bmatrix*" | "vmatrix*"
        | "Vmatrix*" | "array" | "cases" | "dcases" | "dcases*" | "rcases" | "rcases*"
        | "numcases" | "subnumcases" => Some(true),
        "aligned" | "alignedat" | "gathered" | "split" | "multlined" | "gather" | "gather*"
        | "align" | "align*" | "equation" | "equation*" | "multline" | "multline*" | "flalign"
        | "flalign*" | "CD" | "prooftree" | "xymatrix" => Some(false),
        _ => None,
    }
}

fn substitute(def: &MacroDef, args: &[Vec<Token>], cap: usize) -> ExpansionResult<Vec<Token>> {
    // Substitute directly in token space.  Detokenizing and retokenizing here
    // would turn `\alpha` followed by an argument letter into `\alphax`, and
    // would also erase frozen meanings captured by `\let`/`\noexpand`.
    let mut size = usize::from(
        def.parameter_text
            .as_ref()
            .is_some_and(ParameterText::terminal_open),
    );
    let mut i = 0;
    while i < def.body.len() {
        if matches!(def.body[i], Token::Char('#'))
            && let Some(Token::Char(next)) = def.body.get(i + 1)
        {
            if *next == '#' {
                size = size.saturating_add(1);
                i += 2;
                continue;
            }
            if let Some(index) = next.to_digit(10)
                && index > 0
                && (index as usize) <= args.len()
            {
                size = size.saturating_add(
                    args[index as usize - 1]
                        .iter()
                        .map(Token::byte_len)
                        .sum::<usize>(),
                );
                if size > cap {
                    return Err(ExpansionFailure::OutputBytes { limit: cap });
                }
                i += 2;
                continue;
            }
        }
        size = size.saturating_add(def.body[i].byte_len());
        if size > cap {
            return Err(ExpansionFailure::OutputBytes { limit: cap });
        }
        i += 1;
    }
    let mut out = Vec::with_capacity(def.body.len().min(cap).min(4096));
    if def
        .parameter_text
        .as_ref()
        .is_some_and(ParameterText::terminal_open)
    {
        out.push(Token::BeginGroup);
    }
    i = 0;
    while i < def.body.len() {
        if matches!(def.body[i], Token::Char('#'))
            && let Some(Token::Char(next)) = def.body.get(i + 1)
        {
            if *next == '#' {
                out.push(Token::Char('#'));
                i += 2;
                continue;
            }
            if let Some(index) = next.to_digit(10)
                && index > 0
                && (index as usize) <= args.len()
            {
                out.extend(args[index as usize - 1].iter().cloned());
                i += 2;
                continue;
            }
        }
        out.push(def.body[i].clone());
        i += 1;
    }
    Ok(out)
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
            Token::EndEnvironment => {},
            Token::NoExpand(inner) => out.push_str(&token_text(std::slice::from_ref(inner))),
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
            let name_len = crate::latex::control_word_len(&input[after..]);
            if name_len > 0 {
                tokens.push(Token::ControlWord(
                    input[after..after + name_len].to_owned(),
                ));
                i = after + name_len;
                // Horizontal source whitespace terminates a control word; it is
                // not one of the replacement or argument tokens. Keep physical
                // line endings for the parser's paragraph handling.
                while matches!(input.as_bytes().get(i), Some(b' ' | b'\t')) {
                    i += 1;
                }
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

fn render_tokens_bounded(tokens: &[Token], cap: usize) -> ExpansionResult<String> {
    let mut out = String::new();
    let mut previous_nonverb_control_word = false;
    render_tokens_into(tokens, &mut out, &mut previous_nonverb_control_word, cap)?;
    Ok(out)
}

fn render_tokens_into(
    tokens: &[Token],
    out: &mut String,
    previous_nonverb_control_word: &mut bool,
    cap: usize,
) -> ExpansionResult<()> {
    for token in tokens {
        match token {
            Token::Char(c) => {
                if *previous_nonverb_control_word && c.is_ascii_alphabetic() {
                    push_rendered(out, " ", cap)?;
                }
                let mut encoded = [0u8; 4];
                push_rendered(out, c.encode_utf8(&mut encoded), cap)?;
                *previous_nonverb_control_word = false;
            },
            Token::BeginGroup => {
                push_rendered(out, "{", cap)?;
                *previous_nonverb_control_word = false;
            },
            Token::EndGroup => {
                push_rendered(out, "}", cap)?;
                *previous_nonverb_control_word = false;
            },
            Token::ControlWord(name) => {
                push_rendered(out, "\\", cap)?;
                push_rendered(out, name, cap)?;
                *previous_nonverb_control_word = !name.starts_with("verb");
            },
            Token::ControlSymbol(c) => {
                push_rendered(out, "\\", cap)?;
                let mut encoded = [0u8; 4];
                push_rendered(out, c.encode_utf8(&mut encoded), cap)?;
                *previous_nonverb_control_word = false;
            },
            Token::Verbatim(text) => {
                push_rendered(out, text, cap)?;
                *previous_nonverb_control_word = false;
            },
            Token::EndEnvironment => {},
            Token::NoExpand(inner) => {
                render_tokens_into(
                    std::slice::from_ref(inner),
                    out,
                    previous_nonverb_control_word,
                    cap,
                )?;
                *previous_nonverb_control_word = false;
            },
            Token::Frozen(inner) => {
                if let Token::ControlWord(name) = inner.as_ref() {
                    push_rendered(out, "\\", cap)?;
                    push_rendered(out, name, cap)?;
                    *previous_nonverb_control_word = !name.starts_with("verb");
                } else {
                    render_tokens_into(
                        std::slice::from_ref(inner),
                        out,
                        previous_nonverb_control_word,
                        cap,
                    )?;
                    *previous_nonverb_control_word = false;
                }
            },
        }
    }
    Ok(())
}

fn push_rendered(out: &mut String, text: &str, cap: usize) -> ExpansionResult<()> {
    if out.len().saturating_add(text.len()) > cap {
        return Err(ExpansionFailure::OutputBytes { limit: cap });
    }
    out.push_str(text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand_with_limits(input: &str, cap: usize, work_cap: usize) -> ExpansionResult<String> {
        Processor::new_with_limits(cap, work_cap).expand_source(input)
    }

    fn chained_name(index: usize) -> String {
        let mut n = index;
        let mut suffix = String::new();
        loop {
            suffix.push((b'A' + (n % 26) as u8) as char);
            n /= 26;
            if n == 0 {
                break;
            }
        }
        format!("m{suffix}")
    }

    #[test]
    fn expansion_depth_is_a_typed_failure() {
        let mut input = String::new();
        for index in 0..=MAX_EXPANSION_DEPTH {
            let name = chained_name(index);
            input.push_str("\\def\\");
            input.push_str(&name);
            input.push('{');
            if index == MAX_EXPANSION_DEPTH {
                input.push('x');
            } else {
                input.push('\\');
                input.push_str(&chained_name(index + 1));
            }
            input.push('}');
        }
        input.push('\\');
        input.push_str(&chained_name(0));

        assert_eq!(
            expand_with_limits(&input, 4096, 1 << 20),
            Err(ExpansionFailure::Depth {
                limit: MAX_EXPANSION_DEPTH
            })
        );
    }

    #[test]
    fn discarded_selector_branches_consume_work_budget() {
        let discarded = "a".repeat(512);
        let input = format!(r"\@firstoftwo{{{discarded}}}{{x}}");
        assert_eq!(
            expand_with_limits(&input, 4096, 128),
            Err(ExpansionFailure::Work { limit: 128 })
        );
    }

    #[test]
    fn reusable_macros_fit_a_small_work_budget() {
        let input = r"\def\foo{xy}\foo+\foo";
        assert_eq!(expand_with_limits(input, 128, 4096).unwrap(), "xy+xy");
    }

    #[test]
    fn unrelated_tail_tokens_do_not_multiply_macro_work() {
        let tail = "a".repeat(256);
        let input = format!("\\def\\foo{{x}}\\foo+{tail}");
        assert_eq!(
            expand_with_limits(&input, 2048, 1400).unwrap(),
            format!("x+{tail}")
        );
    }

    #[test]
    fn final_control_word_separator_counts_toward_output_limit() {
        let mut processor = Processor::new_with_limits(7, 128);
        processor.seed_builtins();
        let mut queue = VecDeque::from([
            Queued {
                token: Token::ControlWord("alpha".into()),
                depth: 0,
            },
            Queued {
                token: Token::Char('a'),
                depth: 0,
            },
        ]);
        let mut output = Vec::new();
        processor
            .process_queue(&mut queue, &mut output, Mode::Normal)
            .unwrap();
        assert_eq!(
            render_tokens_bounded(&output, 7),
            Err(ExpansionFailure::OutputBytes { limit: 7 })
        );
    }
}
