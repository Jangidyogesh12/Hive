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
fn delete_single_variable() {
    let c = clause_at(r#"MATCH (n) DELETE n"#, 1);
    match c {
        Clause::Delete(dc) => {
            assert_eq!(dc.variables, vec!["n"]);
            assert!(!dc.detach);
        }
        other => panic!("expected Delete clause, got {other:?}"),
    }
}

#[test]
fn delete_multiple_variables() {
    let c = clause_at(r#"MATCH (a)-[r]-(b) DELETE r, a"#, 1);
    match c {
        Clause::Delete(dc) => {
            assert_eq!(dc.variables, vec!["r", "a"]);
            assert!(!dc.detach);
        }
        other => panic!("expected Delete clause, got {other:?}"),
    }
}

#[test]
fn detach_delete() {
    let c = clause_at(r#"MATCH (n) DETACH DELETE n"#, 1);
    match c {
        Clause::Delete(dc) => {
            assert_eq!(dc.variables, vec!["n"]);
            assert!(dc.detach);
        }
        other => panic!("expected Delete clause, got {other:?}"),
    }
}

#[test]
fn detach_delete_multiple() {
    let c = clause_at(r#"MATCH (a)-[r]->(b) DETACH DELETE a, r, b"#, 1);
    match c {
        Clause::Delete(dc) => {
            assert_eq!(dc.variables, vec!["a", "r", "b"]);
            assert!(dc.detach);
        }
        other => panic!("expected Delete clause, got {other:?}"),
    }
}
