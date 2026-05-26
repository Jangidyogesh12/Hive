use crate::query::{
    ast::{Expression, Statement},
    parser::parse,
};

#[test]
fn parse_set_integer_value() {
    let stmt = parse("SET n.age = 31").unwrap();

    match stmt {
        Statement::Set(clause) => {
            assert_eq!(clause.variable, "n");
            assert_eq!(clause.property, "age");
            assert_eq!(clause.value, Expression::Integer(31));
        }
        _ => panic!("expected SET statement"),
    }
}

#[test]
fn parse_set_string_value() {
    let stmt = parse("SET n.name = \"Alice\"").unwrap();

    match stmt {
        Statement::Set(clause) => {
            assert_eq!(clause.variable, "n");
            assert_eq!(clause.property, "name");
            assert_eq!(clause.value, Expression::String("Alice".to_string()));
        }
        _ => panic!("expected SET statement"),
    }
}
