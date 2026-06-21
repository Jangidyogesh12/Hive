use crate::{errors::DbError, query::ast::Expression, value::Value};

/// Evaluates a literal expression into a concrete `Value`.
/// Returns an error for non-literal expressions.
pub fn expression_to_literal(expr: &Expression) -> Result<Value, DbError> {
    match expr {
        Expression::Integer(n) => Ok(Value::Integer(*n)),
        Expression::Float(f) => Ok(Value::Float(*f)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Boolean(b) => Ok(Value::Boolean(*b)),
        _ => Err(DbError::QueryError(format!(
            "Expected a literal value, got: {:?}",
            expr
        ))),
    }
}
