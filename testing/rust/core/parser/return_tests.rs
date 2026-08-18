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
fn return_single_property() {
    let c = clause_at("MATCH (n) RETURN n.name", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.items.len(), 1);
            assert_eq!(rc.items[0].alias, None);
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_with_alias() {
    let c = clause_at("MATCH (n) RETURN n.name AS name", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.items.len(), 1);
            assert_eq!(rc.items[0].alias.as_deref(), Some("name"));
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_multiple_items() {
    let c = clause_at("MATCH (n) RETURN n.name AS name, n.age AS age", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.items.len(), 2);
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_with_order_by_asc() {
    let c = clause_at("MATCH (n) RETURN n.name ORDER BY n.age ASC", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.order_by.len(), 1);
            assert!(!rc.order_by[0].descending);
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_with_order_by_desc() {
    let c = clause_at("MATCH (n) RETURN n.name ORDER BY n.age DESC", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.order_by.len(), 1);
            assert!(rc.order_by[0].descending);
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_with_order_by_default() {
    let c = clause_at("MATCH (n) RETURN n.name ORDER BY n.age", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.order_by.len(), 1);
            assert!(!rc.order_by[0].descending);
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_with_skip() {
    let c = clause_at("MATCH (n) RETURN n.name SKIP 10", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.skip, Some(10));
            assert_eq!(rc.limit, None);
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_with_limit() {
    let c = clause_at("MATCH (n) RETURN n.name LIMIT 5", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.skip, None);
            assert_eq!(rc.limit, Some(5));
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_with_skip_and_limit() {
    let c = clause_at("MATCH (n) RETURN n.name SKIP 10 LIMIT 5", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.skip, Some(10));
            assert_eq!(rc.limit, Some(5));
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_order_skip_limit_together() {
    let c = clause_at(
        r#"MATCH (n) RETURN n.name ORDER BY n.age DESC SKIP 2 LIMIT 3"#,
        1,
    );
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.order_by.len(), 1);
            assert!(rc.order_by[0].descending);
            assert_eq!(rc.skip, Some(2));
            assert_eq!(rc.limit, Some(3));
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_order_by_multiple_keys() {
    let c = clause_at("MATCH (n) RETURN n.name ORDER BY n.age DESC, n.name ASC", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.order_by.len(), 2);
            assert!(rc.order_by[0].descending);
            assert!(!rc.order_by[1].descending);
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_integer_literal() {
    let c = clause_at("MATCH (n) RETURN 42", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.items[0].expression, Expression::Integer(42));
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
#[allow(clippy::approx_constant)]
fn return_float_literal() {
    let c = clause_at("MATCH (n) RETURN 3.14", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.items[0].expression, Expression::Float(3.14));
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_string_literal() {
    let c = clause_at(r#"MATCH (n) RETURN "hello""#, 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(
                rc.items[0].expression,
                Expression::String("hello".to_string())
            );
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_boolean_literal_true() {
    let c = clause_at("MATCH (n) RETURN true", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.items[0].expression, Expression::Boolean(true));
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_boolean_literal_false() {
    let c = clause_at("MATCH (n) RETURN false", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(rc.items[0].expression, Expression::Boolean(false));
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_variable_reference() {
    let c = clause_at("MATCH (n) RETURN n", 1);
    match c {
        Clause::Return(rc) => {
            assert_eq!(
                rc.items[0].expression,
                Expression::Variable("n".to_string())
            );
        }
        other => panic!("expected Return clause, got {other:?}"),
    }
}

#[test]
fn return_property_access() {
    let c = clause_at("MATCH (n) RETURN n.name", 1);
    match c {
        Clause::Return(rc) => match &rc.items[0].expression {
            Expression::Property { variable, property } => {
                assert_eq!(variable, "n");
                assert_eq!(property, "name");
            }
            other => panic!("expected Property expression, got {other:?}"),
        },
        other => panic!("expected Return clause, got {other:?}"),
    }
}
