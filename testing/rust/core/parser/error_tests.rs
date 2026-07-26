use crate::query::error::ParseError;
use crate::query::parser::parse;

#[test]
fn error_empty_input() {
    assert!(parse("").is_err());
}

#[test]
fn error_whitespace_only() {
    assert!(parse("   ").is_err());
}

#[test]
fn error_unsupported_update_keyword() {
    let err = parse("UPDATE (n) SET n.x = 1").unwrap_err();
    match err {
        ParseError::UnexpectedToken { expected, .. } => {
            assert!(
                expected.contains("CREATE") || expected.contains("query clause"),
                "expected clause keyword error, got: {expected}"
            );
        }
        other => panic!("expected UnexpectedToken, got {other:?}"),
    }
}

#[test]
fn error_bare_expression_without_clause() {
    let err = parse("n.name").unwrap_err();
    match err {
        ParseError::UnexpectedToken { .. } => {}
        other => panic!("expected UnexpectedToken for bare expression, got {other:?}"),
    }
}

#[test]
fn error_trailing_garbage() {
    let err = parse("CREATE (n) xyz").unwrap_err();
    match err {
        ParseError::UnexpectedToken { expected, got, .. } => {
            assert!(
                expected.contains("end of input") || got.contains("xyz"),
                "expected end-of-input or xyz error, got expected={expected} got={got}"
            );
        }
        other => panic!("expected UnexpectedToken, got {other:?}"),
    }
}

#[test]
fn error_missing_return_expression() {
    assert!(parse("MATCH (n) RETURN").is_err());
}

#[test]
fn error_missing_set_property() {
    assert!(parse("MATCH (n) SET").is_err());
}

#[test]
fn error_missing_delete_variable() {
    assert!(parse("MATCH (n) DELETE").is_err());
}

#[test]
fn error_unterminated_string() {
    assert!(parse(r#"MATCH (n) SET n.name = "Alice"#).is_err());
}

#[test]
fn error_invalid_relationship_range() {
    let err = parse(r#"MATCH (a)-[*5..2]->(b) RETURN a"#).unwrap_err();
    match err {
        ParseError::Generic { message, .. } => {
            assert!(
                message.contains("min cannot exceed max"),
                "unexpected message: {message}"
            );
        }
        other => panic!("expected Generic error for invalid range, got {other:?}"),
    }
}

#[test]
fn error_missing_arrow_after_bracket() {
    assert!(parse(r#"MATCH (a)-[r:KNOWS](b) RETURN a"#).is_err());
}

#[test]
fn error_missing_relationship_bracket() {
    assert!(parse(r#"MATCH (a)-r:KNOWS->(b) RETURN a"#).is_err());
}
