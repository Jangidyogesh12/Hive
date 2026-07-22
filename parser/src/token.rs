use std::fmt;

/// Token categories produced by the lexer.
///
/// The parser consumes these tokens instead of reading raw characters directly.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    // Keywords
    Create,
    Match,
    Delete,
    Merge,
    Set,
    Where,
    Return,
    As,
    And,
    Or,
    Not,
    True,
    False,
    Order,
    By,
    Limit,
    Skip,
    Asc,
    Desc,
    Detach,

    // Symbols
    LParen,    // (
    RParen,    // )
    LBrace,    // {
    RBrace,    // }
    LBracket,  // [
    RBracket,  // ]
    Comma,     // ,
    Colon,     // :
    Dot,       // .
    Semicolon, // ;

    // Operators
    Eq,         // =
    Neq,        // <>
    Gt,         // >
    Gte,        // >=
    Lt,         // <
    Lte,        // <=
    ArrowRight, // ->
    ArrowLeft,  // <-
    Dash,       // -
    Star,       // *
    DotDot,     // ..

    // Literals & Identifiers
    Integer(i64),
    Float(f64),
    StringLit(String),
    Identifier(String),

    // Special
    Eof,
}

impl TokenType {
    /// Returns true for literal value tokens that can become AST expressions.
    pub fn is_literal(&self) -> bool {
        matches!(
            self,
            TokenType::Integer(_) | TokenType::Float(_) | TokenType::StringLit(_)
        )
    }
}

impl fmt::Display for TokenType {
    /// Formats tokens for parser errors and debug output.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TokenType::Create => write!(f, "CREATE"),
            TokenType::Match => write!(f, "MATCH"),
            TokenType::Delete => write!(f, "DELETE"),
            TokenType::Merge => write!(f, "MERGE"),
            TokenType::Set => write!(f, "SET"),
            TokenType::Where => write!(f, "WHERE"),
            TokenType::Return => write!(f, "RETURN"),
            TokenType::As => write!(f, "AS"),
            TokenType::And => write!(f, "AND"),
            TokenType::Or => write!(f, "OR"),
            TokenType::Not => write!(f, "NOT"),
            TokenType::True => write!(f, "true"),
            TokenType::False => write!(f, "false"),
            TokenType::Order => write!(f, "ORDER"),
            TokenType::By => write!(f, "BY"),
            TokenType::Limit => write!(f, "LIMIT"),
            TokenType::Skip => write!(f, "SKIP"),
            TokenType::Asc => write!(f, "ASC"),
            TokenType::Desc => write!(f, "DESC"),
            TokenType::Detach => write!(f, "DETACH"),
            TokenType::LParen => write!(f, "("),
            TokenType::RParen => write!(f, ")"),
            TokenType::LBrace => write!(f, "{{"),
            TokenType::RBrace => write!(f, "}}"),
            TokenType::LBracket => write!(f, "["),
            TokenType::RBracket => write!(f, "]"),
            TokenType::Comma => write!(f, ","),
            TokenType::Colon => write!(f, ":"),
            TokenType::Dot => write!(f, "."),
            TokenType::Semicolon => write!(f, ";"),
            TokenType::Eq => write!(f, "="),
            TokenType::Neq => write!(f, "<>"),
            TokenType::Gt => write!(f, ">"),
            TokenType::Gte => write!(f, ">="),
            TokenType::Lt => write!(f, "<"),
            TokenType::Lte => write!(f, "<="),
            TokenType::ArrowRight => write!(f, "->"),
            TokenType::ArrowLeft => write!(f, "<-"),
            TokenType::Dash => write!(f, "-"),
            TokenType::Star => write!(f, "*"),
            TokenType::DotDot => write!(f, ".."),
            TokenType::Integer(n) => write!(f, "{}", n),
            TokenType::Float(n) => write!(f, "{}", n),
            TokenType::StringLit(s) => write!(f, "\"{}\"", s),
            TokenType::Identifier(s) => write!(f, "{}", s),
            TokenType::Eof => write!(f, "EOF"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token<'a> {
    /// Classified token kind, such as `Match`, `Identifier`, or `Integer`.
    pub kind: TokenType,
    /// Byte range where this token appeared in the original query.
    pub span: Span,
    /// Original query slice for this token. This borrows from lexer input.
    pub text: &'a str,
}

/// Byte range inside the original query text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Inclusive start byte offset.
    pub start: usize,
    /// Exclusive end byte offset.
    pub end: usize,
}

impl Span {
    /// Creates a new byte span.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Returns span length in bytes.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Returns true when the span covers no bytes.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Converts this span to the diagnostics type used by `miette`.
    pub fn to_miette(&self) -> miette::SourceSpan {
        miette::SourceSpan::from(self.start)
    }
}
