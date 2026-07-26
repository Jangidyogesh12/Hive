use crate::query::ast::*;
use crate::query::parser::parse;

fn stmt(input: &str) -> Statement {
    parse(input).unwrap_or_else(|e| panic!("parse failed for `{input}`: {e}"))
}

fn clause(input: &str) -> Clause {
    stmt(input).clauses.into_iter().next().unwrap()
}

#[test]
fn create_node_with_label_and_properties() {
    let c = clause(r#"CREATE (n:Person {name: "Alice", age: 30})"#);
    match c {
        Clause::Create(Pattern::Node(node)) => {
            assert_eq!(node.variable.as_deref(), Some("n"));
            assert_eq!(node.label.as_deref(), Some("Person"));
            assert_eq!(node.properties.len(), 2);
            assert_eq!(
                *node.properties.get("name").unwrap(),
                Expression::String("Alice".to_string())
            );
            assert_eq!(
                *node.properties.get("age").unwrap(),
                Expression::Integer(30)
            );
        }
        other => panic!("expected Node pattern, got {other:?}"),
    }
}

#[test]
fn create_node_label_only() {
    let c = clause("CREATE (:Person)");
    match c {
        Clause::Create(Pattern::Node(node)) => {
            assert_eq!(node.variable, None);
            assert_eq!(node.label.as_deref(), Some("Person"));
            assert!(node.properties.is_empty());
        }
        other => panic!("expected Node pattern, got {other:?}"),
    }
}

#[test]
fn create_node_empty_parens() {
    let c = clause("CREATE (n)");
    match c {
        Clause::Create(Pattern::Node(node)) => {
            assert_eq!(node.variable.as_deref(), Some("n"));
            assert_eq!(node.label, None);
            assert!(node.properties.is_empty());
        }
        other => panic!("expected Node pattern, got {other:?}"),
    }
}

#[test]
fn create_path_outgoing_edge() {
    let c = clause(r#"CREATE (a:Person)-[:KNOWS]->(b:Person)"#);
    match c {
        Clause::Create(Pattern::Path(path)) => {
            assert_eq!(path.segments.len(), 1);
            assert_eq!(path.start.label.as_deref(), Some("Person"));
            let seg = &path.segments[0];
            assert_eq!(seg.relationship.rel_type.as_deref(), Some("KNOWS"));
            assert_eq!(seg.relationship.direction, Direction::Outgoing);
            assert_eq!(seg.node.label.as_deref(), Some("Person"));
        }
        other => panic!("expected Path pattern, got {other:?}"),
    }
}

#[test]
fn create_path_incoming_edge() {
    let c = clause(r#"CREATE (a:Person)<-[:KNOWS]-(b:Person)"#);
    match c {
        Clause::Create(Pattern::Path(path)) => {
            let seg = &path.segments[0];
            assert_eq!(seg.relationship.direction, Direction::Incoming);
        }
        other => panic!("expected Path pattern, got {other:?}"),
    }
}

#[test]
fn create_path_undirected_edge() {
    let c = clause(r#"CREATE (a:Person)-[:KNOWS]-(b:Person)"#);
    match c {
        Clause::Create(Pattern::Path(path)) => {
            let seg = &path.segments[0];
            assert_eq!(seg.relationship.direction, Direction::Undirected);
        }
        other => panic!("expected Path pattern, got {other:?}"),
    }
}

#[test]
fn create_path_with_edge_variable_and_properties() {
    let c = clause(r#"CREATE (a)-[r:KNOWS {since: 2020}]-(b)"#);
    match c {
        Clause::Create(Pattern::Path(path)) => {
            let seg = &path.segments[0];
            assert_eq!(seg.relationship.variable.as_deref(), Some("r"));
            assert_eq!(seg.relationship.rel_type.as_deref(), Some("KNOWS"));
            assert_eq!(
                seg.relationship.properties.get("since").unwrap(),
                &Expression::Integer(2020)
            );
        }
        other => panic!("expected Path pattern, got {other:?}"),
    }
}

#[test]
fn create_path_multi_hop() {
    let c = clause(r#"CREATE (a)-[:KNOWS]->(b)-[:LIKES]->(c)"#);
    match c {
        Clause::Create(Pattern::Path(path)) => {
            assert_eq!(path.segments.len(), 2);
            assert_eq!(
                path.segments[0].relationship.rel_type.as_deref(),
                Some("KNOWS")
            );
            assert_eq!(
                path.segments[1].relationship.rel_type.as_deref(),
                Some("LIKES")
            );
        }
        other => panic!("expected Path pattern, got {other:?}"),
    }
}

#[test]
fn create_edge_with_properties() {
    let c = clause(r#"CREATE (a)-[r:KNOWS {since: 2020, trust: true}]->(b)"#);
    match c {
        Clause::Create(Pattern::Path(path)) => {
            let rel = &path.segments[0].relationship;
            assert_eq!(rel.properties.len(), 2);
            assert_eq!(
                rel.properties.get("since").unwrap(),
                &Expression::Integer(2020)
            );
            assert_eq!(
                rel.properties.get("trust").unwrap(),
                &Expression::Boolean(true)
            );
        }
        other => panic!("expected Path pattern, got {other:?}"),
    }
}

#[test]
fn create_node_integer_property() {
    let c = clause(r#"CREATE (n:Item {temp: 5})"#);
    match c {
        Clause::Create(Pattern::Node(node)) => {
            assert!(node.properties.contains_key("temp"));
            assert_eq!(
                node.properties.get("temp").unwrap(),
                &Expression::Integer(5)
            );
        }
        other => panic!("expected Node pattern, got {other:?}"),
    }
}
