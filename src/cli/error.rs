use miette::Diagnostic;
use thiserror::Error;

/// Exit codes following sysexits(3).
pub mod exit {
    #[allow(dead_code)]
    pub const OK: i32 = 0;
    pub const USAGE: i32 = 64; // EX_USAGE
    pub const DATAERR: i32 = 65; // EX_DATAERR
    pub const NOINPUT: i32 = 66; // EX_NOINPUT
    pub const SOFTWARE: i32 = 70; // EX_SOFTWARE
    pub const IOERR: i32 = 74; // EX_IOERR
}

/// CLI-facing error type with rich diagnostics via `miette`.
#[derive(Debug, Error, Diagnostic)]
pub enum CliError {
    #[error("{message}")]
    #[diagnostic(code(mathema::parse))]
    ParseWithSource {
        message: String,
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("here")]
        span: miette::SourceSpan,
    },

    #[error("{0}")]
    #[diagnostic(code(mathema::conversion))]
    Conversion(String),

    #[error("could not detect input format for '{path}'")]
    #[diagnostic(code(mathema::detect), help("specify --from <format>"))]
    DetectionFailed { path: String },

    #[error("file not found: {0}")]
    #[diagnostic(code(mathema::io))]
    FileNotFound(String),

    #[error("I/O error: {0}")]
    #[diagnostic(code(mathema::io))]
    Io(String),

    #[error("{0}")]
    #[diagnostic(code(mathema::render))]
    Render(String),

    #[error("{0}")]
    #[diagnostic(code(mathema::unsupported))]
    Unsupported(String),
}

impl CliError {
    /// Map each variant to a sysexits(3) exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::ParseWithSource { .. } | CliError::Conversion(_) => exit::DATAERR,
            CliError::DetectionFailed { .. } | CliError::Unsupported(_) => exit::USAGE,
            CliError::FileNotFound(_) => exit::NOINPUT,
            CliError::Io(_) => exit::IOERR,
            CliError::Render(_) => exit::SOFTWARE,
        }
    }

    /// Convert a library-level `mathema::Error` into a CLI error, attaching
    /// source context when span information is available.
    pub fn from_mathema(err: mathema::Error, source: &str, origin: &str) -> Self {
        match err {
            mathema::Error::LatexParse {
                message,
                span: Some(span),
            } => CliError::ParseWithSource {
                message,
                src: miette::NamedSource::new(origin, source.to_owned()),
                span: (span.start, span.end.saturating_sub(span.start)).into(),
            },
            mathema::Error::LatexParse {
                message,
                span: None,
            } => CliError::Conversion(message),
            mathema::Error::MathmlParse { message } => CliError::Conversion(message),
            mathema::Error::OmmlParse { message } => CliError::Conversion(message),
            mathema::Error::Xml(e) => CliError::Conversion(e.to_string()),
            mathema::Error::Render { message } => CliError::Render(message),
            mathema::Error::Unsupported(msg) => CliError::Unsupported(msg),
            mathema::Error::InputTooLarge { size, limit } => CliError::Conversion(format!(
                "input size {size} bytes exceeds configured limit of {limit} bytes"
            )),
            mathema::Error::Io(e) => CliError::Io(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_parse() {
        let err = CliError::ParseWithSource {
            message: "bad".into(),
            src: miette::NamedSource::new("test", "x".to_string()),
            span: (0, 1).into(),
        };
        assert_eq!(err.exit_code(), exit::DATAERR);
    }

    #[test]
    fn exit_code_conversion() {
        assert_eq!(CliError::Conversion("x".into()).exit_code(), exit::DATAERR);
    }

    #[test]
    fn exit_code_detection_failed() {
        let err = CliError::DetectionFailed { path: "f".into() };
        assert_eq!(err.exit_code(), exit::USAGE);
    }

    #[test]
    fn exit_code_file_not_found() {
        assert_eq!(
            CliError::FileNotFound("f".into()).exit_code(),
            exit::NOINPUT
        );
    }

    #[test]
    fn exit_code_io() {
        assert_eq!(CliError::Io("x".into()).exit_code(), exit::IOERR);
    }

    #[test]
    fn exit_code_render() {
        assert_eq!(CliError::Render("x".into()).exit_code(), exit::SOFTWARE);
    }

    #[test]
    fn exit_code_unsupported() {
        assert_eq!(CliError::Unsupported("x".into()).exit_code(), exit::USAGE);
    }

    #[test]
    fn from_mathema_latex_with_span() {
        let err = mathema::Error::LatexParse {
            message: "unexpected token".into(),
            span: Some(mathema::error::Span::new(2, 5)),
        };
        let cli_err = CliError::from_mathema(err, "x + y", "<stdin>");
        match cli_err {
            CliError::ParseWithSource { message, .. } => {
                assert_eq!(message, "unexpected token");
            },
            other => panic!("expected ParseWithSource, got {other:?}"),
        }
    }

    #[test]
    fn from_mathema_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        let err = mathema::Error::Io(io_err);
        let cli_err = CliError::from_mathema(err, "", "f");
        assert_eq!(cli_err.exit_code(), exit::IOERR);
    }
}
