use crate::query::{
    ast::{Direction, Expression, Pattern, Statement},
    parser::parse,
};

#[test]
fn parse_create_node_with_multiple_property_types() {
    let stmt = parse("CREATE (n:Person {name: \"Alice\", age: 30, active: true, score: 1.5})").unwrap();

    match stmt {
        Statement::Create(Pattern::Node(node)) => {
            assert_eq!(node.variable, Some("n".to_string()));
            assert_eq!(node.label, Some("Person".to_string()));
            assert_eq!(node.properties.get("name"), Some(&Expression::String("Alice".to_string())));
            assert_eq!(node.properties.get("age"), Some(&Expression::Integer(30)));
            assert_eq!(node.properties.get("active"), Some(&Expression::Boolean(true)));
            assert_eq!(node.properties.get("score"), Some(&Expression::Float(1.5)));
        }
        _ => panic!("expected CREATE node pattern"),
    }
}

#[test]
fn parse_create_relationship_with_variable_type_direction_and_props() {
    let stmt = parse(
        "CREATE (a:Person {name: \"Alice\"})-[r:KNOWS {since: 2024}]->(b:Person {name: \"Bob\"})",
    )
    .unwrap();

    match stmt {
        Statement::Create(Pattern::Edge(left, rel, right)) => {
            assert_eq!(left.variable, Some("a".to_string()));
            assert_eq!(left.label, Some("Person".to_string()));
            assert_eq!(left.properties.get("name"), Some(&Expression::String("Alice".to_string())));

            assert_eq!(rel.variable, Some("r".to_string()));
            assert_eq!(rel.rel_type, Some("KNOWS".to_string()));
            assert_eq!(rel.direction, Direction::Outgoing);
            assert_eq!(rel.properties.get("since"), Some(&Expression::Integer(2024)));

            assert_eq!(right.variable, Some("b".to_string()));
            assert_eq!(right.label, Some("Person".to_string()));
            assert_eq!(right.properties.get("name"), Some(&Expression::String("Bob".to_string())));
        }
        _ => panic!("expected CREATE relationship pattern"),
    }
}
