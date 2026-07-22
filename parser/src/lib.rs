//! Parser crate for Hive's Cypher-like query language.
//!
//! The parser pipeline is:
//! raw query text -> lexer tokens -> parser AST -> core planner/executor.

/// AST types produced after parsing a query.
pub mod ast;
/// Parse errors with source spans for useful diagnostics.
pub mod error;
/// Lexer that converts raw query text into tokens.
pub mod lexer;
/// Recursive-descent parser that converts tokens into AST clauses.
pub mod parser;
/// Token and source-span definitions shared by the lexer and parser.
pub mod token;
