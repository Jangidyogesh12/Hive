use crate::query::ast::*;
use crate::query::parser::parse;

fn stmt(input: &str) -> Statement {
    parse(input).unwrap_or_else(|e| panic!("parse failed for `{input}`: {e}"))
}

fn clause(input: &str) -> Clause {
    stmt(input).clauses.into_iter().next().unwrap()
}

#[test]
fn match_node_with_label() {
    let c = clause("MATCH (n:Person) RETURN n.name");
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Node(node) => {
                assert_eq!(node.variable.as_deref(), Some("n"));
                assert_eq!(node.label.as_deref(), Some("Person"));
            }
            other => panic!("expected Node pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}

#[test]
fn match_path_outgoing() {
    let c = clause(r#"MATCH (a)-[r:KNOWS]->(b) RETURN a.name"#);
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Path(path) => {
                assert_eq!(path.segments.len(), 1);
                assert_eq!(
                    path.segments[0].relationship.direction,
                    Direction::Outgoing
                );
            }
            other => panic!("expected Path pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}

#[test]
fn match_path_incoming() {
    let c = clause(r#"MATCH (a)<-[r:KNOWS]-(b) RETURN a.name"#);
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Path(path) => {
                assert_eq!(
                    path.segments[0].relationship.direction,
                    Direction::Incoming
                );
            }
            other => panic!("expected Path pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}

#[test]
fn match_path_undirected() {
    let c = clause(r#"MATCH (a)-[r:KNOWS]-(b) RETURN a.name"#);
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Path(path) => {
                assert_eq!(
                    path.segments[0].relationship.direction,
                    Direction::Undirected
                );
            }
            other => panic!("expected Path pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}

#[test]
fn match_variable_length_unbounded() {
    let c = clause(r#"MATCH (a)-[*]->(b) RETURN a"#);
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Path(path) => {
                let hops = path.segments[0].relationship.hops.as_ref().unwrap();
                assert_eq!(hops.min_hops, None);
                assert_eq!(hops.max_hops, None);
            }
            other => panic!("expected Path pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}

#[test]
fn match_variable_length_bounded() {
    let c = clause(r#"MATCH (a)-[*2..5]->(b) RETURN a"#);
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Path(path) => {
                let hops = path.segments[0].relationship.hops.as_ref().unwrap();
                assert_eq!(hops.min_hops, Some(2));
                assert_eq!(hops.max_hops, Some(5));
            }
            other => panic!("expected Path pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}

#[test]
fn match_variable_length_open_ended() {
    let c = clause(r#"MATCH (a)-[*2..]->(b) RETURN a"#);
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Path(path) => {
                let hops = path.segments[0].relationship.hops.as_ref().unwrap();
                assert_eq!(hops.min_hops, Some(2));
                assert_eq!(hops.max_hops, None);
            }
            other => panic!("expected Path pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}

#[test]
fn match_path_no_rel_variable() {
    let c = clause(r#"MATCH (a)-[:KNOWS]->(b) RETURN a"#);
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Path(path) => {
                assert_eq!(path.segments[0].relationship.variable, None);
            }
            other => panic!("expected Path pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}

#[test]
fn match_path_no_rel_type() {
    let c = clause(r#"MATCH (a)-[r]->(b) RETURN a"#);
    match c {
        Clause::Match(mc) => match mc.pattern {
            Pattern::Path(path) => {
                assert_eq!(path.segments[0].relationship.rel_type, None);
            }
            other => panic!("expected Path pattern, got {other:?}"),
        },
        other => panic!("expected Match clause, got {other:?}"),
    }
}
