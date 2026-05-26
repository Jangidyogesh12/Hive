use crate::query::{ast::Statement, parser::parse};

#[test]
fn parse_delete_variable() {
    let stmt = parse("DELETE n").unwrap();

    match stmt {
        Statement::Delete(variable) => assert_eq!(variable, "n"),
        _ => panic!("expected DELETE statement"),
    }
}
