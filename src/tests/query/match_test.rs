use crate::query::{
    ast::{BinaryOp, Direction, Expression, Pattern, Statement, UnaryOp},
    parser::parse,
};

#[test]
fn parse_match_node_with_where_and_alias_return() {
    let stmt = parse("MATCH (n:Person {active: true}) WHERE n.age > 25 RETURN n.name AS person_name, n.age").unwrap();

    match stmt {
        Statement::Match(clause) => {
            match &clause.pattern {
                Pattern::Node(node) => {
                    assert_eq!(node.variable, Some("n".to_string()));
                    assert_eq!(node.label, Some("Person".to_string()));
                    assert_eq!(node.properties.get("active"), Some(&Expression::Boolean(true)));
                }
                _ => panic!("expected node pattern"),
            }

            assert!(clause.where_clause.is_some());
            assert_eq!(clause.return_clause.items.len(), 2);
            assert_eq!(clause.return_clause.items[0].alias, Some("person_name".to_string()));
            assert_eq!(clause.return_clause.items[1].alias, None);
        }
        _ => panic!("expected MATCH statement"),
    }
}

#[test]
fn parse_match_relationship_incoming_direction() {
    let stmt = parse("MATCH (n:Person)<-[r:KNOWS]-(m:Person) RETURN n, m").unwrap();

    match stmt {
        Statement::Match(clause) => {
            assert_eq!(clause.return_clause.items.len(), 2);
            match &clause.pattern {
                Pattern::Path(path) => {
                    assert_eq!(path.segments.len(), 1);
                    let rel = &path.segments[0].relationship;
                    assert_eq!(rel.variable, Some("r".to_string()));
                    assert_eq!(rel.rel_type, Some("KNOWS".to_string()));
                    assert_eq!(rel.direction, Direction::Incoming);
                    assert_eq!(rel.hops, None);
                }
                _ => panic!("expected edge pattern"),
            }
        }
        _ => panic!("expected MATCH statement"),
    }
}

#[test]
fn parse_match_relationship_variable_hops() {
    let stmt = parse("MATCH (n:Person)-[:KNOWS*1..3]->(m:Person) RETURN n, m").unwrap();

    match stmt {
        Statement::Match(clause) => match &clause.pattern {
            Pattern::Path(path) => {
                assert_eq!(path.segments.len(), 1);
                let rel = &path.segments[0].relationship;
                let hops = rel.hops.as_ref().expect("expected hops range");
                assert_eq!(hops.min_hops, Some(1));
                assert_eq!(hops.max_hops, Some(3));
            }
            _ => panic!("expected edge pattern"),
        },
        _ => panic!("expected MATCH statement"),
    }
}

#[test]
fn parse_match_where_boolean_expression_tree() {
    let stmt = parse("MATCH (n:Person) WHERE n.age > 18 AND n.active = true OR n.age = 0 RETURN n").unwrap();

    match stmt {
        Statement::Match(clause) => {
            let where_clause = clause.where_clause.expect("where clause required");
            match where_clause.condition {
                Expression::BinaryOp { op: BinaryOp::Or, .. } => {}
                _ => panic!("expected OR at top level"),
            }
        }
        _ => panic!("expected MATCH statement"),
    }
}

#[test]
fn parse_match_relationship_undirected_direction() {
    let stmt = parse("MATCH (n:Person)-[r:KNOWS]-(m:Person) RETURN n, m").unwrap();

    match stmt {
        Statement::Match(clause) => {
            assert_eq!(clause.return_clause.items.len(), 2);
            match &clause.pattern {
                Pattern::Path(path) => {
                    assert_eq!(path.segments.len(), 1);
                    let rel = &path.segments[0].relationship;
                    assert_eq!(rel.variable, Some("r".to_string()));
                    assert_eq!(rel.rel_type, Some("KNOWS".to_string()));
                    assert_eq!(rel.direction, Direction::Undirected);
                }
                _ => panic!("expected edge pattern"),
            }
        }
        _ => panic!("expected MATCH statement"),
    }
}

#[test]
fn parse_match_complex_path_pattern() {
    let stmt = parse("MATCH (a)-[:KNOWS]->(b)-[:WORKS_AT]->(c) RETURN a, b, c").unwrap();

    match stmt {
        Statement::Match(clause) => match &clause.pattern {
            Pattern::Path(path) => {
                assert_eq!(path.start.variable, Some("a".to_string()));
                assert_eq!(path.segments.len(), 2);

                assert_eq!(
                    path.segments[0].relationship.rel_type,
                    Some("KNOWS".to_string())
                );
                assert_eq!(path.segments[0].node.variable, Some("b".to_string()));

                assert_eq!(
                    path.segments[1].relationship.rel_type,
                    Some("WORKS_AT".to_string())
                );
                assert_eq!(path.segments[1].node.variable, Some("c".to_string()));
            }
            _ => panic!("expected path pattern"),
        },
        _ => panic!("expected MATCH statement"),
    }
}

#[test]
fn parse_match_where_not_expression_tree() {
    let stmt = parse("MATCH (n:Person) WHERE NOT n.age >= 25 RETURN n").unwrap();

    match stmt {
        Statement::Match(clause) => {
            let where_clause = clause.where_clause.expect("where clause required");
            match where_clause.condition {
                Expression::UnaryOp {
                    op: UnaryOp::Not, ..
                } => {}
                _ => panic!("expected NOT unary op at top level"),
            }
        }
        _ => panic!("expected MATCH statement"),
    }
}

#[test]
fn parse_match_where_double_not_expression_tree() {
    let stmt = parse("MATCH (n:Person) WHERE NOT NOT n.age >= 25 RETURN n").unwrap();

    match stmt {
        Statement::Match(clause) => {
            let where_clause = clause.where_clause.expect("where clause required");
            match where_clause.condition {
                Expression::UnaryOp {
                    op: UnaryOp::Not,
                    expr,
                } => match *expr {
                    Expression::UnaryOp {
                        op: UnaryOp::Not, ..
                    } => {}
                    _ => panic!("expected nested NOT unary op"),
                },
                _ => panic!("expected NOT unary op at top level"),
            }
        }
        _ => panic!("expected MATCH statement"),
    }
}
