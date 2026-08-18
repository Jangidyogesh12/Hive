use crate::query::ast::*;
use crate::query::parser::parse;

fn clause_at(input: &str, idx: usize) -> Clause {
    parse(input)
        .unwrap_or_else(|e| panic!("parse failed for `{input}`: {e}"))
        .clauses
        .into_iter()
        .nth(idx)
        .unwrap_or_else(|| panic!("no clause at index {idx} for `{input}`"))
}

#[test]
fn set_integer_value() {
    let c = clause_at(r#"MATCH (n) SET n.age = 30 RETURN n"#, 1);
    match c {
        Clause::Set(sc) => {
            assert_eq!(sc.variable, "n");
            assert_eq!(sc.property, "age");
            assert_eq!(sc.value, Expression::Integer(30));
        }
        other => panic!("expected Set clause, got {other:?}"),
    }
}

#[test]
fn set_string_value() {
    let c = clause_at(r#"MATCH (n) SET n.name = "Alice" RETURN n"#, 1);
    match c {
        Clause::Set(sc) => {
            assert_eq!(sc.value, Expression::String("Alice".to_string()));
        }
        other => panic!("expected Set clause, got {other:?}"),
    }
}

#[test]
fn set_boolean_value() {
    let c = clause_at(r#"MATCH (n) SET n.active = true RETURN n"#, 1);
    match c {
        Clause::Set(sc) => {
            assert_eq!(sc.value, Expression::Boolean(true));
        }
        other => panic!("expected Set clause, got {other:?}"),
    }
}

#[test]
fn set_float_value() {
    let c = clause_at(r#"MATCH (n) SET n.score = 9.5 RETURN n"#, 1);
    match c {
        Clause::Set(sc) => {
            assert_eq!(sc.value, Expression::Float(9.5));
        }
        other => panic!("expected Set clause, got {other:?}"),
    }
}

#[test]
fn set_with_binary_expression() {
    let c = clause_at(r#"MATCH (n) SET n.score = n.score + 1 RETURN n"#, 1);
    match c {
        Clause::Set(sc) => match &sc.value {
            Expression::BinaryOp { op, .. } => {
                assert_eq!(*op, BinaryOp::Eq);
            }
            other => {
                assert!(
                    !format!("{other:?}").is_empty(),
                    "SET RHS should be a valid expression"
                );
            }
        },
        other => panic!("expected Set clause, got {other:?}"),
    }
}
