use crate::query::{
    ast::{Expression, Pattern, Statement},
    parser::parse,
};

#[test]
fn parse_merge_node_with_properties() {
    let stmt = parse("MERGE (n:Person {name: \"Alice\", age: 30})").unwrap();

    match stmt {
        Statement::Merge(Pattern::Node(node)) => {
            assert_eq!(node.variable, Some("n".to_string()));
            assert_eq!(node.label, Some("Person".to_string()));
            assert_eq!(
                node.properties.get("name"),
                Some(&Expression::String("Alice".to_string()))
            );
            assert_eq!(node.properties.get("age"), Some(&Expression::Integer(30)));
        }
        _ => panic!("expected MERGE node pattern"),
    }
}
