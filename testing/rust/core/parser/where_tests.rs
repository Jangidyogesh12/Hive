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
fn where_equality() {
    let c = clause_at(r#"MATCH (n) WHERE n.name = "Alice" RETURN n"#, 1);
    match c {
        Clause::Where(expr) => match expr {
            Expression::BinaryOp { op, .. } => assert_eq!(op, BinaryOp::Eq),
            other => panic!("expected BinaryOp, got {other:?}"),
        },
        other => panic!("expected Where clause, got {other:?}"),
    }
}

#[test]
fn where_not_equal() {
    let c = clause_at(r#"MATCH (n) WHERE n.name <> "Bob" RETURN n"#, 1);
    match c {
        Clause::Where(expr) => match expr {
            Expression::BinaryOp { op, .. } => assert_eq!(op, BinaryOp::Neq),
            other => panic!("expected BinaryOp, got {other:?}"),
        },
        other => panic!("expected Where clause, got {other:?}"),
    }
}

#[test]
fn where_greater_than() {
    let c = clause_at(r#"MATCH (n) WHERE n.age > 21 RETURN n"#, 1);
    match c {
        Clause::Where(expr) => match expr {
            Expression::BinaryOp { op, .. } => assert_eq!(op, BinaryOp::Gt),
            other => panic!("expected BinaryOp, got {other:?}"),
        },
        other => panic!("expected Where clause, got {other:?}"),
    }
}

#[test]
fn where_less_equal() {
    let c = clause_at(r#"MATCH (n) WHERE n.age <= 65 RETURN n"#, 1);
    match c {
        Clause::Where(expr) => match expr {
            Expression::BinaryOp { op, .. } => assert_eq!(op, BinaryOp::Lte),
            other => panic!("expected BinaryOp, got {other:?}"),
        },
        other => panic!("expected Where clause, got {other:?}"),
    }
}

#[test]
fn where_and() {
    let c = clause_at(r#"MATCH (n) WHERE n.age > 18 AND n.name = "A" RETURN n"#, 1);
    match c {
        Clause::Where(expr) => match expr {
            Expression::BinaryOp { op, .. } => assert_eq!(op, BinaryOp::And),
            other => panic!("expected BinaryOp(And), got {other:?}"),
        },
        other => panic!("expected Where clause, got {other:?}"),
    }
}

#[test]
fn where_or() {
    let c = clause_at(r#"MATCH (n) WHERE n.name = "A" OR n.name = "B" RETURN n"#, 1);
    match c {
        Clause::Where(expr) => match expr {
            Expression::BinaryOp { op, .. } => assert_eq!(op, BinaryOp::Or),
            other => panic!("expected BinaryOp(Or), got {other:?}"),
        },
        other => panic!("expected Where clause, got {other:?}"),
    }
}

#[test]
fn where_not() {
    let c = clause_at(r#"MATCH (n) WHERE NOT n.active RETURN n"#, 1);
    match c {
        Clause::Where(expr) => match expr {
            Expression::UnaryOp { op, .. } => assert_eq!(op, UnaryOp::Not),
            other => panic!("expected UnaryOp(Not), got {other:?}"),
        },
        other => panic!("expected Where clause, got {other:?}"),
    }
}

#[test]
fn where_complex_precedence() {
    let c = clause_at(r#"MATCH (n) WHERE n.a = 1 AND n.b = 2 OR n.c = 3 RETURN n"#, 1);
    match c {
        Clause::Where(expr) => match expr {
            Expression::BinaryOp { op, left, .. } => {
                assert_eq!(op, BinaryOp::Or);
                match left.as_ref() {
                    Expression::BinaryOp { op: lop, .. } => assert_eq!(*lop, BinaryOp::And),
                    other => panic!("expected AND on left, got {other:?}"),
                }
            }
            other => panic!("expected BinaryOp, got {other:?}"),
        },
        other => panic!("expected Where clause, got {other:?}"),
    }
}

#[test]
fn match_path_with_where() {
    let s = parse("MATCH (n:Person) WHERE n.age >= 30 RETURN n").unwrap();
    assert_eq!(s.clauses.len(), 3);
    assert!(matches!(s.clauses[1], Clause::Where(_)));
}
