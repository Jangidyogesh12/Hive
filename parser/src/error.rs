use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
/// Errors returned while lexing or parsing query text.
pub enum ParseError {
    /// Parser expected more input but reached the end of the query.
    #[error("Unexpected end of input")]
    UnexpectedEOF {
        #[label("input ends here")]
        span: SourceSpan,
    },

    /// Lexer found a character that is not valid in the query language.
    #[error("Unexpected character: '{ch}'")]
    UnexpectedCharacter {
        /// The invalid character.
        ch: char,
        #[label("unexpected character")]
        span: SourceSpan,
    },

    /// Parser found a token that does not fit the expected grammar.
    #[error("Unexpected token: {got}, expected one of: {expected}")]
    UnexpectedToken {
        /// Human-readable expected token or grammar item.
        expected: String,
        /// Actual token found.
        got: String,
        #[label("unexpected token")]
        span: SourceSpan,
    },

    /// String literal started with `"` but never closed.
    #[error("Unterminated string literal")]
    UnterminatedString {
        #[label("string starts here")]
        span: SourceSpan,
    },

    /// Numeric literal could not be parsed into an integer or float.
    #[error("Invalid number: {text}")]
    InvalidNumber {
        /// Original numeric text.
        text: String,
        #[label("invalid number")]
        span: SourceSpan,
    },

    /// Generic parser error used for grammar-specific validation failures.
    #[error("{message}")]
    Generic {
        /// Human-readable error message.
        message: String,
        #[label]
        span: SourceSpan,
    },
}

impl ParseError {
    /// Returns the source span attached to this parse error.
    pub fn span(&self) -> &SourceSpan {
        match self {
            ParseError::UnexpectedEOF { span } => span,
            ParseError::UnexpectedCharacter { span, .. } => span,
            ParseError::UnexpectedToken { span, .. } => span,
            ParseError::UnterminatedString { span } => span,
            ParseError::InvalidNumber { span, .. } => span,
            ParseError::Generic { span, .. } => span,
        }
    }
}
