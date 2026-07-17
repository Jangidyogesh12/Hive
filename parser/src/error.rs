use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum ParseError {
    #[error("Unexpected end of input")]
    UnexpectedEOF {
        #[label("input ends here")]
        span: SourceSpan,
    },

    #[error("Unexpected character: '{ch}'")]
    UnexpectedCharacter {
        ch: char,
        #[label("unexpected character")]
        span: SourceSpan,
    },

    #[error("Unexpected token: {got}, expected one of: {expected}")]
    UnexpectedToken {
        expected: String,
        got: String,
        #[label("unexpected token")]
        span: SourceSpan,
    },

    #[error("Unterminated string literal")]
    UnterminatedString {
        #[label("string starts here")]
        span: SourceSpan,
    },

    #[error("Invalid number: {text}")]
    InvalidNumber {
        text: String,
        #[label("invalid number")]
        span: SourceSpan,
    },

    #[error("{message}")]
    Generic {
        message: String,
        #[label]
        span: SourceSpan,
    },
}

impl ParseError {
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
