use crate::query::result::QueryResult;
use crate::value::Value;

#[test]
fn query_result_formats_ascii_table() {
    let result = QueryResult::new(
        vec!["person_name".to_string(), "age".to_string()],
        vec![
            vec![Value::String("Alice".to_string()), Value::Integer(30)],
            vec![Value::String("Bob".to_string()), Value::Integer(28)],
        ],
    );

    let table = result.to_ascii_table();

    assert!(table.contains("| person_name | age |"));
    assert!(table.contains("| Alice       | 30  |"));
    assert!(table.contains("| Bob         | 28  |"));
}

#[test]
fn query_result_formats_empty_rows_with_header() {
    let result = QueryResult::new(vec!["n".to_string()], vec![]);
    let table = result.to_ascii_table();

    assert!(table.contains("| n |"));
    assert_eq!(table.lines().count(), 4);
}

#[test]
fn query_result_formats_null_value() {
    let result = QueryResult::new(vec!["v".to_string()], vec![vec![Value::Null]]);
    let table = result.to_ascii_table();

    assert!(table.contains("| v    |"));
    assert!(table.contains("| NULL |"));
}
