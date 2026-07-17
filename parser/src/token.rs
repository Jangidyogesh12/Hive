use std::fmt;

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
    Eq,          // =
    Neq,         // <>
    Gt,          // >
    Gte,         // >=
    Lt,          // <
    Lte,         // <=
    ArrowRight,  // ->
    ArrowLeft,   // <-
    Dash,        // -
    Star,        // *
    DotDot,      // ..

    // Literals & Identifiers
    Integer(i64),
    Float(f64),
    StringLit(String),
    Identifier(String),

    // Special
    Eof,
}

impl TokenType {
    pub fn is_literal(&self) -> bool {
        matches!(self, TokenType::Integer(_) | TokenType::Float(_) | TokenType::StringLit(_))
    }
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    pub kind: TokenType,
    pub span: Span,
    pub text: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn to_miette(&self) -> miette::SourceSpan {
        miette::SourceSpan::from(self.start)
    }
}
