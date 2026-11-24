//! Undefined LaTeX control sequences kept in the AST.
//!
//! The LaTeX reader keeps a command it does not know (`\foo`) as
//! [`MathNode::UndefinedCommand`] instead of turning its name into
//! identifier letters, the way KaTeX does with `throwOnError: false`. The
//! brace groups written directly after it stay with it as its possible
//! arguments, since its arity is unknown.
//!
//! The MathML writer uses MathJax's
//! `<merror><mtext>\foo</mtext><mrow>a</mrow></merror>`, which the MathML
//! reader reads back; the renderer shows the name upright in
//! [`UNDEFINED_COMMAND_COLOR`](crate::ast::undefined::UNDEFINED_COMMAND_COLOR); OMML, UnicodeMath and MTEF write it as text.
//!
//! # LaTeX output and safety
//!
//! The LaTeX output may be fed to TeX. The guarantee is this: **with the
//! default configuration ([`UndefinedCommands::Escape`]), an unknown command
//! never comes out as live LaTeX.** The writer spells it as visible, inert
//! text and shows its groups as literal braces,
//! `\mathord{\text{\textbackslash{}foo}\{a\}}`, which reads back as that
//! same text, so the output is stable. (Before, it became identifier
//! letters, `fooa`: just as inert, but lossy.)
//!
//! [`UndefinedCommands::Keep`] writes the command back live (`\foo{a}`) for
//! output compiled where it is defined. That mode is best effort, not safe
//! for untrusted input compiled with file, Lua or shell access: a command
//! unknown here may be a package's file reader or code runner. As defence
//! in depth only, two rules hold in every mode:
//!
//! - A name on a fixed list of dangerous primitives (in this module's source)
//!   (`\input`, `\write`, `\catcode`, `\usepackage`, `\directlua`, …) is
//!   never written live in any mode: every writer spells it as the inert
//!   text above, and the LaTeX reader reports it as not allowed. The list
//!   cannot be complete, so it is not a guarantee.
//! - A writer writes the node live, and the MathML reader builds it from an
//!   `<merror>`, only for a name the LaTeX reader itself keeps as undefined;
//!   a node built through the API or serde with any other name (`\alpha`,
//!   `}\input{x`) is written as text.
//!
//! [`UndefinedCommands::Escape`]: crate::UndefinedCommands::Escape
//! [`UndefinedCommands::Keep`]: crate::UndefinedCommands::Keep

use super::MathNode;
use super::arena::AstArena;
use super::compact::{CompactNode, NodeId};

/// The colour an undefined control sequence is rendered in (KaTeX's
/// `errorColor`).
pub const UNDEFINED_COMMAND_COLOR: &str = "#cc0000";

/// Whether `spelling` is one complete control word made of letters: a
/// backslash followed by ASCII letters. (The LaTeX reader rejects KaTeX's
/// internal `\@` names outright, so they never become this node.)
pub(crate) fn is_control_word(spelling: &str) -> bool {
    spelling
        .strip_prefix('\\')
        .is_some_and(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphabetic()))
}

/// Whether `spelling` names a known TeX, LaTeX or package command that is
/// never written back as a live command, even with
/// [`UndefinedCommands::Keep`](crate::UndefinedCommands::Keep): it reads or
/// writes files, runs Lua or shell code, changes how TeX reads its input,
/// defines macros, loads code, or ends or loops the run. Defence in depth
/// only: the list cannot be complete.
pub(crate) fn is_dangerous_command(spelling: &str) -> bool {
    matches!(
        spelling.strip_prefix('\\').unwrap_or(spelling),
        "input"
            | "include"
            | "InputIfFileExists"
            | "IfFileExists"
            | "openin"
            | "openout"
            | "closein"
            | "closeout"
            | "read"
            | "readline"
            | "write"
            | "immediate"
            | "message"
            | "special"
            | "catcode"
            | "lccode"
            | "uccode"
            | "mathcode"
            | "csname"
            | "endcsname"
            | "expandafter"
            | "usepackage"
            | "RequirePackage"
            | "documentclass"
            | "directlua"
            | "latelua"
            | "luaexec"
            | "ShellEscape"
            | "pdfprimitive"
            | "primitive"
            | "scantokens"
            | "everyjob"
            | "everymath"
            | "everydisplay"
            | "everypar"
            | "def"
            | "gdef"
            | "edef"
            | "xdef"
            | "let"
            | "futurelet"
            | "global"
            | "outer"
            | "long"
            | "newcommand"
            | "renewcommand"
            | "providecommand"
            | "DeclareRobustCommand"
            | "newenvironment"
            | "renewenvironment"
            | "NewDocumentCommand"
            | "RenewDocumentCommand"
            | "ProvideDocumentCommand"
            | "DeclareDocumentCommand"
            | "NewDocumentEnvironment"
            | "chardef"
            | "mathchardef"
            | "countdef"
            | "toksdef"
            | "toks"
            | "newwrite"
            | "newread"
            | "lstinputlisting"
            | "verbatiminput"
            | "VerbatimInput"
            | "inputminted"
            | "pdffiledump"
            | "pdfmdfivesum"
            | "filesize"
            | "pdffilesize"
            | "pdffilemoddate"
            | "font"
            | "subfile"
            | "subimport"
            | "import"
            | "includepdf"
            | "includegraphics"
            | "usetikzlibrary"
            | "usepgflibrary"
            | "LoadClass"
            | "includeonly"
            | "luadirect"
            | "luacode"
            | "ExplSyntaxOn"
            | "ExplSyntaxOff"
            | "makeatletter"
            | "makeatother"
            | "loop"
            | "repeat"
            | "shipout"
            | "output"
            | "endinput"
            | "errmessage"
            | "batchmode"
            | "nonstopmode"
            | "scrollmode"
            | "errorstopmode"
            | "dump"
            | "jobname"
    )
}

/// Whether an undefined command spelled `spelling` may be written as a live
/// control word: a plain control word, not dangerous, that the LaTeX reader
/// itself keeps as an undefined command. Anything else (a name built through
/// the API, such as `\alpha`, `\left` or `}\input{x`) is written as escaped
/// text.
pub(crate) fn writes_as_command(spelling: &str) -> bool {
    crate::latex::parse::reads_as_undefined_command(spelling)
}

/// Whether `spelling` (`\right`) names a command the LaTeX reader reads only
/// in its own context: closing a `\left`, ending an environment, or starting
/// a table cell. Elsewhere it is kept like an undefined command, reported as
/// misplaced, and written back in braces with its groups, so that it is not
/// read in a context where it means something (`{\end{x}}` in an array).
pub(crate) fn is_context_command(spelling: &str) -> bool {
    matches!(
        spelling,
        r"\right" | r"\end" | r"\multicolumn" | r"\cellcolor" | r"\rowcolor" | r"\columncolor"
    )
}

impl MathNode {
    /// The spelling (`\foo`) of an undefined control sequence kept by the
    /// LaTeX reader, or `None` for any other node.
    #[must_use]
    pub fn as_undefined_command(&self) -> Option<&str> {
        match self {
            MathNode::UndefinedCommand { name, .. } => Some(name),
            _ => None,
        }
    }
}

impl AstArena {
    /// The spelling (`\foo`) of the undefined control sequence at `id`, or
    /// `None` when that node is anything else.
    #[must_use]
    #[inline]
    pub fn undefined_command(&self, id: NodeId) -> Option<&str> {
        match *self.node(id) {
            CompactNode::UndefinedCommand { name, .. } => Some(self.str_val(name)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_word_shapes() {
        for word in [r"\foo", r"\smallint", r"\A"] {
            assert!(is_control_word(word), "{word}");
        }
        for other in [
            "",
            "foo",
            r"\",
            r"\@foo",
            r"\,",
            r"\foo1",
            r"\foo bar",
            r"\a@b",
        ] {
            assert!(!is_control_word(other), "{other}");
        }
    }

    #[test]
    fn context_commands() {
        assert!(is_context_command(r"\end"));
        assert!(!is_context_command(r"\foo"));
    }
}
