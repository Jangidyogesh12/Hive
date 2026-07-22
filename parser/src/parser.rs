use std::collections::HashMap;

use crate::ast::{
    BinaryOp, Clause, DeleteClause, Direction, Expression, MatchClause, NodePattern, OrderItem,
    PathPattern, PathSegment, Pattern, RelationshipLength, RelationshipPattern, ReturnClause,
    ReturnItem, SetClause, Statement, UnaryOp,
};
use crate::error::ParseError;
use crate::lexer::Lexer;
use crate::token::{Span, Token, TokenType};

/// Recursive-descent parser for Hive's Cypher-like query language.
///
/// The parser owns a lexer and keeps one current token for lookahead.
pub struct Parser<'a> {
    /// Token source over the original query text.
    lexer: Lexer<'a>,
    /// Current lookahead token.
    current: Token<'a>,
}

impl<'a> Parser<'a> {
    /// Creates a parser and loads the first token.
    pub fn new(input: &'a str) -> Self {
        let mut lexer = Lexer::new(input);
        let current = lexer.next_token().unwrap_or(Token {
            kind: TokenType::Eof,
            span: Span::new(0, 0),
            text: "",
        });
        Self { lexer, current }
    }

    /// Consumes the current token and loads the next one.
    fn advance(&mut self) -> Token<'a> {
        let prev = self.current.clone();
        self.current = self.lexer.next_token().unwrap_or_else(|_| Token {
            kind: TokenType::Eof,
            span: Span::new(self.current.span.end, self.current.span.end),
            text: "",
        });
        prev
    }

    /// Returns the current token kind without consuming it.
    fn peek(&self) -> &TokenType {
        &self.current.kind
    }

    /// Consumes a token of the expected kind or returns a parse error.
    fn expect(&mut self, expected: TokenType) -> Result<Token<'a>, ParseError> {
        if std::mem::discriminant(&self.current.kind) == std::mem::discriminant(&expected) {
            Ok(self.advance())
        } else {
            Err(ParseError::UnexpectedToken {
                expected: expected.to_string(),
                got: self.current.kind.to_string(),
                span: self.current.span.to_miette(),
            })
        }
    }

    /// Consumes an identifier token and returns its string value.
    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match &self.current.kind {
            TokenType::Identifier(s) => {
                let s = s.clone();
                self.advance();
                Ok(s)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "identifier".to_string(),
                got: self.current.kind.to_string(),
                span: self.current.span.to_miette(),
            }),
        }
    }

    /// Checks whether the current token has the same variant as `kind`.
    fn at(&self, kind: &TokenType) -> bool {
        std::mem::discriminant(&self.current.kind) == std::mem::discriminant(kind)
    }

    /// Parses a full query into an ordered clause pipeline.
    pub fn parse_statement(&mut self) -> Result<Statement, ParseError> {
        let mut clauses = Vec::new();

        while !self.at(&TokenType::Eof) && !self.at(&TokenType::Semicolon) {
            let clause = match self.peek() {
                TokenType::Create => self.parse_create(),
                TokenType::Match => self.parse_match(),
                TokenType::Where => self.parse_where(),
                TokenType::Delete | TokenType::Detach => self.parse_delete(),
                TokenType::Merge => self.parse_merge(),
                TokenType::Set => self.parse_set(),
                TokenType::Return => self.parse_return().map(Clause::Return),
                _ => Err(ParseError::UnexpectedToken {
                    expected: "CREATE, MATCH, WHERE, DELETE, MERGE, SET, or RETURN".to_string(),
                    got: self.current.kind.to_string(),
                    span: self.current.span.to_miette(),
                }),
            }?;
            clauses.push(clause);
        }

        if clauses.is_empty() {
            return Err(ParseError::UnexpectedToken {
                expected: "query clause".to_string(),
                got: self.current.kind.to_string(),
                span: self.current.span.to_miette(),
            });
        }

        if self.at(&TokenType::Semicolon) {
            self.advance();
        }

        if !self.at(&TokenType::Eof) {
            return Err(ParseError::UnexpectedToken {
                expected: "end of input".to_string(),
                got: self.current.kind.to_string(),
                span: self.current.span.to_miette(),
            });
        }

        Ok(Statement { clauses })
    }

    /// Parses `CREATE <pattern>`.
    fn parse_create(&mut self) -> Result<Clause, ParseError> {
        self.advance(); // consume CREATE
        let pattern = self.parse_pattern()?;
        Ok(Clause::Create(pattern))
    }

    /// Parses `MATCH <pattern>`.
    fn parse_match(&mut self) -> Result<Clause, ParseError> {
        self.advance(); // consume MATCH
        let pattern = self.parse_pattern()?;
        Ok(Clause::Match(MatchClause { pattern }))
    }

    /// Parses `WHERE <expression>`.
    fn parse_where(&mut self) -> Result<Clause, ParseError> {
        self.advance(); // consume WHERE
        Ok(Clause::Where(self.parse_expression()?))
    }

    /// Parses `DELETE a, b` and `DETACH DELETE a, b`.
    fn parse_delete(&mut self) -> Result<Clause, ParseError> {
        let detach = if self.at(&TokenType::Detach) {
            self.advance();
            true
        } else {
            false
        };
        self.expect(TokenType::Delete)?;
        let mut variables = vec![self.expect_ident()?];
        while self.at(&TokenType::Comma) {
            self.advance();
            variables.push(self.expect_ident()?);
        }
        Ok(Clause::Delete(DeleteClause { variables, detach }))
    }

    /// Parses `MERGE <pattern>`.
    fn parse_merge(&mut self) -> Result<Clause, ParseError> {
        self.advance(); // consume MERGE
        let pattern = self.parse_pattern()?;
        Ok(Clause::Merge(pattern))
    }

    /// Parses `SET variable.property = expression`.
    fn parse_set(&mut self) -> Result<Clause, ParseError> {
        self.advance(); // consume SET
        let variable = self.expect_ident()?;
        self.expect(TokenType::Dot)?;
        let property = self.expect_ident()?;
        self.expect(TokenType::Eq)?;
        let value = self.parse_expression()?;

        Ok(Clause::Set(SetClause {
            variable,
            property,
            value,
        }))
    }

    /// Parses a node-only pattern or a path pattern.
    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let start = self.parse_node_pattern()?;
        let mut segments = Vec::new();

        while self.is_rel_start() {
            let relationship = self.parse_relationship_pattern()?;
            let node = self.parse_node_pattern()?;
            segments.push(PathSegment { relationship, node });
        }

        if segments.is_empty() {
            Ok(Pattern::Node(start))
        } else {
            Ok(Pattern::Path(PathPattern { start, segments }))
        }
    }

    /// Returns true when the current token can begin a relationship segment.
    fn is_rel_start(&self) -> bool {
        matches!(
            self.peek(),
            TokenType::ArrowLeft | TokenType::ArrowRight | TokenType::Dash
        )
    }

    /// Parses `(variable:Label {properties})` node syntax.
    fn parse_node_pattern(&mut self) -> Result<NodePattern, ParseError> {
        self.expect(TokenType::LParen)?;

        let variable = if matches!(self.peek(), TokenType::Identifier(_)) {
            Some(self.expect_ident()?)
        } else {
            None
        };

        let label = if self.at(&TokenType::Colon) {
            self.advance(); // consume ':'
            Some(self.expect_ident()?)
        } else {
            None
        };

        let properties = if self.at(&TokenType::LBrace) {
            self.parse_property_map()?
        } else {
            HashMap::new()
        };

        self.expect(TokenType::RParen)?;

        Ok(NodePattern {
            variable,
            label,
            properties,
        })
    }

    /// Parses relationship syntax such as `-[r:TYPE]->`, `<-[r]-`, or `-[r]-`.
    fn parse_relationship_pattern(&mut self) -> Result<RelationshipPattern, ParseError> {
        // Parse leading direction tokens
        let has_leading_arrow = self.at(&TokenType::ArrowLeft);
        if has_leading_arrow {
            self.advance(); // consume '<-'
        } else if self.at(&TokenType::Dash) {
            self.advance(); // consume '-'
        } else {
            return Err(ParseError::UnexpectedToken {
                expected: "<-, or -".to_string(),
                got: self.current.kind.to_string(),
                span: self.current.span.to_miette(),
            });
        }

        self.expect(TokenType::LBracket)?;

        let mut variable = None;
        let mut rel_type = None;
        let mut properties = HashMap::new();
        let mut hops = None;

        if matches!(self.peek(), TokenType::Identifier(_)) {
            variable = Some(self.expect_ident()?);
        }

        if self.at(&TokenType::Colon) {
            self.advance(); // consume ':'
            rel_type = Some(self.expect_ident()?);
        }

        if self.at(&TokenType::Star) {
            hops = Some(self.parse_rel_length()?);
        }

        if self.at(&TokenType::LBrace) {
            properties = self.parse_property_map()?;
        }

        self.expect(TokenType::RBracket)?;

        // Parse trailing direction tokens to determine final direction
        let direction = if has_leading_arrow {
            // Incoming: already consumed '<-', now consume trailing '-'
            self.expect(TokenType::Dash)?;
            Direction::Incoming
        } else if self.at(&TokenType::ArrowRight) {
            self.advance(); // consume '->'
            Direction::Outgoing
        } else if self.at(&TokenType::Dash) {
            self.advance(); // consume '-'
            Direction::Undirected
        } else {
            return Err(ParseError::UnexpectedToken {
                expected: "-> or -".to_string(),
                got: self.current.kind.to_string(),
                span: self.current.span.to_miette(),
            });
        };

        Ok(RelationshipPattern {
            variable,
            rel_type,
            direction,
            hops,
            properties,
        })
    }

    /// Parses variable-length relationship bounds after `*`.
    fn parse_rel_length(&mut self) -> Result<RelationshipLength, ParseError> {
        self.expect(TokenType::Star)?;

        if !matches!(self.peek(), TokenType::Integer(_)) && !self.at(&TokenType::DotDot) {
            return Ok(RelationshipLength {
                min_hops: None,
                max_hops: None,
            });
        }

        let min_hops = if self.at(&TokenType::DotDot) {
            None
        } else {
            match self.advance().kind {
                TokenType::Integer(n) => Some(n as u32),
                _ => unreachable!(),
            }
        };

        self.expect(TokenType::DotDot)?;

        let max_hops = if matches!(self.peek(), TokenType::Integer(_)) {
            match self.advance().kind {
                TokenType::Integer(n) => Some(n as u32),
                _ => unreachable!(),
            }
        } else {
            None
        };

        if let (Some(min), Some(max)) = (min_hops, max_hops)
            && min > max
        {
            return Err(ParseError::Generic {
                message: "Invalid relationship range: min cannot exceed max".to_string(),
                span: self.current.span.to_miette(),
            });
        }

        Ok(RelationshipLength { min_hops, max_hops })
    }

    /// Parses `{key: expression, ...}` property maps.
    fn parse_property_map(&mut self) -> Result<HashMap<String, Expression>, ParseError> {
        self.expect(TokenType::LBrace)?;
        let mut map = HashMap::new();

        if !self.at(&TokenType::RBrace) {
            loop {
                let key = self.expect_ident()?;
                self.expect(TokenType::Colon)?;
                let value = self.parse_expression()?;
                map.insert(key, value);

                if !self.at(&TokenType::Comma) {
                    break;
                }
                self.advance(); // consume ','
            }
        }

        self.expect(TokenType::RBrace)?;
        Ok(map)
    }

    /// Parses `RETURN` projections plus optional `ORDER BY`, `SKIP`, and `LIMIT`.
    fn parse_return(&mut self) -> Result<ReturnClause, ParseError> {
        self.expect(TokenType::Return)?;

        let mut items = Vec::new();
        loop {
            let expression = self.parse_expression()?;
            let alias = if self.at(&TokenType::As) {
                self.advance(); // consume AS
                Some(self.expect_ident()?)
            } else {
                None
            };
            items.push(ReturnItem { expression, alias });

            if !self.at(&TokenType::Comma) {
                break;
            }
            self.advance(); // consume ','
        }

        let mut order_by = Vec::new();
        if self.at(&TokenType::Order) {
            self.advance();
            self.expect(TokenType::By)?;
            loop {
                let expression = self.parse_expression()?;
                let descending = if self.at(&TokenType::Desc) {
                    self.advance();
                    true
                } else {
                    if self.at(&TokenType::Asc) {
                        self.advance();
                    }
                    false
                };
                order_by.push(OrderItem {
                    expression,
                    descending,
                });
                if !self.at(&TokenType::Comma) {
                    break;
                }
                self.advance();
            }
        }

        let mut skip = None;
        let mut limit = None;
        loop {
            if self.at(&TokenType::Skip) {
                self.advance();
                skip = Some(self.expect_usize()?);
            } else if self.at(&TokenType::Limit) {
                self.advance();
                limit = Some(self.expect_usize()?);
            } else {
                break;
            }
        }

        Ok(ReturnClause {
            items,
            order_by,
            skip,
            limit,
        })
    }

    /// Consumes a non-negative integer token as `usize`.
    fn expect_usize(&mut self) -> Result<usize, ParseError> {
        match self.advance().kind {
            TokenType::Integer(n) if n >= 0 => Ok(n as usize),
            got => Err(ParseError::UnexpectedToken {
                expected: "non-negative integer".to_string(),
                got: got.to_string(),
                span: self.current.span.to_miette(),
            }),
        }
    }

    /// Parses an expression starting at the lowest precedence level.
    fn parse_expression(&mut self) -> Result<Expression, ParseError> {
        self.parse_or_expr()
    }

    /// Parses `OR` expressions.
    fn parse_or_expr(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_and_expr()?;

        while self.at(&TokenType::Or) {
            self.advance();
            let right = self.parse_and_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parses `AND` expressions.
    fn parse_and_expr(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_not_expr()?;

        while self.at(&TokenType::And) {
            self.advance();
            let right = self.parse_not_expr()?;
            left = Expression::BinaryOp {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Parses one or more leading `NOT` operators.
    fn parse_not_expr(&mut self) -> Result<Expression, ParseError> {
        let mut not_count = 0;
        while self.at(&TokenType::Not) {
            self.advance();
            not_count += 1;
        }

        let expr = self.parse_comparison()?;

        let mut result = expr;
        for _ in 0..not_count {
            result = Expression::UnaryOp {
                op: UnaryOp::Not,
                expr: Box::new(result),
            };
        }

        Ok(result)
    }

    /// Parses comparison expressions such as `a.age >= 30`.
    fn parse_comparison(&mut self) -> Result<Expression, ParseError> {
        let left = self.parse_atom()?;

        let op = match self.peek() {
            TokenType::Eq => Some(BinaryOp::Eq),
            TokenType::Neq => Some(BinaryOp::Neq),
            TokenType::Gt => Some(BinaryOp::Gt),
            TokenType::Gte => Some(BinaryOp::Gte),
            TokenType::Lt => Some(BinaryOp::Lt),
            TokenType::Lte => Some(BinaryOp::Lte),
            _ => None,
        };

        if let Some(op) = op {
            self.advance();
            let right = self.parse_atom()?;
            Ok(Expression::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            })
        } else {
            Ok(left)
        }
    }

    /// Parses a literal, variable, property access, or parenthesized expression.
    fn parse_atom(&mut self) -> Result<Expression, ParseError> {
        match self.peek().clone() {
            TokenType::True => {
                self.advance();
                Ok(Expression::Boolean(true))
            }
            TokenType::False => {
                self.advance();
                Ok(Expression::Boolean(false))
            }
            TokenType::Integer(n) => {
                self.advance();
                Ok(Expression::Integer(n))
            }
            TokenType::Float(n) => {
                self.advance();
                Ok(Expression::Float(n))
            }
            TokenType::StringLit(s) => {
                self.advance();
                Ok(Expression::String(s))
            }
            TokenType::Identifier(_) => {
                let name = self.expect_ident()?;
                if self.at(&TokenType::Dot) {
                    self.advance(); // consume '.'
                    let property = self.expect_ident()?;
                    Ok(Expression::Property {
                        variable: name,
                        property,
                    })
                } else {
                    Ok(Expression::Variable(name))
                }
            }
            TokenType::LParen => {
                self.advance(); // consume '('
                let expr = self.parse_expression()?;
                self.expect(TokenType::RParen)?;
                Ok(expr)
            }
            _ => Err(ParseError::UnexpectedToken {
                expected: "expression".to_string(),
                got: self.current.kind.to_string(),
                span: self.current.span.to_miette(),
            }),
        }
    }
}

/// Convenience function for parsing one query string.
pub fn parse(input: &str) -> Result<Statement, ParseError> {
    let mut parser = Parser::new(input);
    parser.parse_statement()
}
