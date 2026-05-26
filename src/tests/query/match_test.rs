use crate::query::{
    ast::{BinaryOp, Direction, Expression, Pattern, Statement},
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
                Pattern::Edge(_, rel, _) => {
                    assert_eq!(rel.variable, Some("r".to_string()));
                    assert_eq!(rel.rel_type, Some("KNOWS".to_string()));
                    assert_eq!(rel.direction, Direction::Incoming);
                }
                _ => panic!("expected edge pattern"),
            }
        }
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
