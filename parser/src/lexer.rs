use crate::error::ParseError;
use crate::token::{Span, Token, TokenType};

/// Converts raw query text into a stream of tokens.
///
/// The lexer borrows the input string so each token can keep a cheap `text`
/// slice pointing back into the original query.
pub struct Lexer<'a> {
    /// Original query text being tokenized.
    input: &'a str,
    /// Current byte offset into `input`.
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Creates a lexer at the beginning of `input`.
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    /// Returns the current character without consuming it.
    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    /// Returns a character ahead of the current position without consuming it.
    fn peek_ahead(&self, offset: usize) -> Option<char> {
        self.input[self.pos..].chars().nth(offset)
    }

    /// Consumes and returns the current character, advancing by its UTF-8 width.
    fn advance(&mut self) -> Option<char> {
        let ch = self.input[self.pos..].chars().next()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    /// Skips whitespace and `//` line comments before reading the next token.
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.advance();
                }
                Some('/') if self.peek_ahead(1) == Some('/') => {
                    while let Some(ch) = self.peek() {
                        if ch == '\n' {
                            break;
                        }
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    /// Reads a double-quoted string literal and handles simple escape sequences.
    fn read_string(&mut self, start: usize) -> Result<Token<'a>, ParseError> {
        self.advance(); // consume opening quote
        let mut value = String::new();

        loop {
            match self.peek() {
                Some('"') => {
                    self.advance();
                    return Ok(Token {
                        kind: TokenType::StringLit(value),
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    });
                }
                Some('\\') => {
                    self.advance();
                    match self.advance() {
                        Some('n') => value.push('\n'),
                        Some('t') => value.push('\t'),
                        Some('\\') => value.push('\\'),
                        Some('"') => value.push('"'),
                        Some(ch) => {
                            value.push('\\');
                            value.push(ch);
                        }
                        None => {
                            return Err(ParseError::UnterminatedString {
                                span: Span::new(start, self.pos).to_miette(),
                            });
                        }
                    }
                }
                Some(ch) => {
                    self.advance();
                    value.push(ch);
                }
                None => {
                    return Err(ParseError::UnterminatedString {
                        span: Span::new(start, self.pos).to_miette(),
                    });
                }
            }
        }
    }

    /// Reads an integer or floating-point literal.
    fn read_number(&mut self, start: usize) -> Result<Token<'a>, ParseError> {
        let mut is_float = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else if ch == '.' && !is_float {
                if self.peek_ahead(1).is_some_and(|c| c.is_ascii_digit()) {
                    is_float = true;
                    self.advance(); // consume '.'
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let text = &self.input[start..self.pos];

        if is_float {
            let value: f64 = text.parse().map_err(|_| ParseError::InvalidNumber {
                text: text.to_string(),
                span: Span::new(start, self.pos).to_miette(),
            })?;
            Ok(Token {
                kind: TokenType::Float(value),
                span: Span::new(start, self.pos),
                text,
            })
        } else {
            let value: i64 = text.parse().map_err(|_| ParseError::InvalidNumber {
                text: text.to_string(),
                span: Span::new(start, self.pos).to_miette(),
            })?;
            Ok(Token {
                kind: TokenType::Integer(value),
                span: Span::new(start, self.pos),
                text,
            })
        }
    }

    /// Reads an identifier and classifies it as a keyword when it matches one.
    fn read_identifier(&mut self, start: usize) -> Token<'a> {
        while let Some(ch) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let text = &self.input[start..self.pos];
        let kind = match text.to_uppercase().as_str() {
            "CREATE" => TokenType::Create,
            "MATCH" => TokenType::Match,
            "DELETE" => TokenType::Delete,
            "MERGE" => TokenType::Merge,
            "SET" => TokenType::Set,
            "WHERE" => TokenType::Where,
            "RETURN" => TokenType::Return,
            "AS" => TokenType::As,
            "AND" => TokenType::And,
            "OR" => TokenType::Or,
            "NOT" => TokenType::Not,
            "TRUE" => TokenType::True,
            "FALSE" => TokenType::False,
            "ORDER" => TokenType::Order,
            "BY" => TokenType::By,
            "LIMIT" => TokenType::Limit,
            "SKIP" => TokenType::Skip,
            "ASC" => TokenType::Asc,
            "DESC" => TokenType::Desc,
            "DETACH" => TokenType::Detach,
            _ => TokenType::Identifier(text.to_string()),
        };

        Token {
            kind,
            span: Span::new(start, self.pos),
            text,
        }
    }

    /// Reads and returns the next token from the input.
    pub fn next_token(&mut self) -> Result<Token<'a>, ParseError> {
        self.skip_whitespace_and_comments();

        let start = self.pos;

        let Some(ch) = self.peek() else {
            return Ok(Token {
                kind: TokenType::Eof,
                span: Span::new(start, start),
                text: &self.input[start..start],
            });
        };

        match ch {
            '(' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::LParen,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            ')' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::RParen,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            '{' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::LBrace,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            '}' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::RBrace,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            '[' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::LBracket,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            ']' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::RBracket,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            ',' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::Comma,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            ':' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::Colon,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            '.' => {
                self.advance();
                if self.peek() == Some('.') {
                    self.advance();
                    Ok(Token {
                        kind: TokenType::DotDot,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                } else {
                    Ok(Token {
                        kind: TokenType::Dot,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                }
            }
            ';' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::Semicolon,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            '=' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::Eq,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            '>' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token {
                        kind: TokenType::Gte,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                } else {
                    Ok(Token {
                        kind: TokenType::Gt,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                }
            }
            '<' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token {
                        kind: TokenType::Lte,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                } else if self.peek() == Some('>') {
                    self.advance();
                    Ok(Token {
                        kind: TokenType::Neq,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                } else if self.peek() == Some('-') {
                    self.advance();
                    Ok(Token {
                        kind: TokenType::ArrowLeft,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                } else {
                    Ok(Token {
                        kind: TokenType::Lt,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                }
            }
            '-' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    Ok(Token {
                        kind: TokenType::ArrowRight,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                } else {
                    Ok(Token {
                        kind: TokenType::Dash,
                        span: Span::new(start, self.pos),
                        text: &self.input[start..self.pos],
                    })
                }
            }
            '*' => {
                self.advance();
                Ok(Token {
                    kind: TokenType::Star,
                    span: Span::new(start, self.pos),
                    text: &self.input[start..self.pos],
                })
            }
            '"' => self.read_string(start),
            c if c.is_ascii_digit() => self.read_number(start),
            c if c.is_ascii_alphabetic() || c == '_' => Ok(self.read_identifier(start)),
            _ => Err(ParseError::UnexpectedCharacter {
                ch,
                span: Span::new(start, start + ch.len_utf8()).to_miette(),
            }),
        }
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = Result<Token<'a>, ParseError>;

    /// Allows callers to iterate over lexer tokens until EOF.
    fn next(&mut self) -> Option<Self::Item> {
        let token = self.next_token();
        match &token {
            Ok(t) if t.kind == TokenType::Eof => None,
            Ok(_) => Some(token),
            Err(_) => Some(token),
        }
    }
}
