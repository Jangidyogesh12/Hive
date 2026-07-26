use crate::query::ast::*;
use crate::query::parser::parse;

fn clause(input: &str) -> Clause {
    parse(input)
        .unwrap_or_else(|e| panic!("parse failed for `{input}`: {e}"))
        .clauses
        .into_iter()
        .next()
        .unwrap()
}

#[test]
fn merge_node() {
    let c = clause(r#"MERGE (n:Person {email: "a@b.com"})"#);
    match c {
        Clause::Merge(Pattern::Node(node)) => {
            assert_eq!(node.variable.as_deref(), Some("n"));
            assert_eq!(node.label.as_deref(), Some("Person"));
            assert_eq!(
                node.properties.get("email").unwrap(),
                &Expression::String("a@b.com".to_string())
            );
        }
        other => panic!("expected Merge(Node), got {other:?}"),
    }
}

#[test]
fn merge_path() {
    let c = clause(r#"MERGE (a)-[:KNOWS]->(b)"#);
    match c {
        Clause::Merge(Pattern::Path(path)) => {
            assert_eq!(path.segments.len(), 1);
            assert_eq!(
                path.segments[0].relationship.rel_type.as_deref(),
                Some("KNOWS")
            );
        }
        other => panic!("expected Merge(Path), got {other:?}"),
    }
}
