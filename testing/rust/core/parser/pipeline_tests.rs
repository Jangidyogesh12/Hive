use crate::query::parser::parse;

fn assert_clause_count(input: &str, n: usize) {
    let s = parse(input).unwrap_or_else(|e| panic!("parse failed for `{input}`: {e}"));
    assert_eq!(
        s.clauses.len(),
        n,
        "expected {n} clauses for `{input}`, got {}",
        s.clauses.len()
    );
}

#[test]
fn create_return_pipeline() {
    assert_clause_count(r#"CREATE (n:Person) RETURN n.name"#, 2);
}

#[test]
fn match_where_set_return_pipeline() {
    assert_clause_count(
        r#"MATCH (n:Person) WHERE n.age >= 30 SET n.active = true RETURN n.name"#,
        4,
    );
}

#[test]
fn match_delete_return_pipeline() {
    assert_clause_count(r#"MATCH (n:Temp) DELETE n RETURN 1"#, 3);
}

#[test]
fn match_detach_delete_pipeline() {
    assert_clause_count(r#"MATCH (n:Temp) DETACH DELETE n RETURN 1"#, 3);
}

#[test]
fn merge_return_pipeline() {
    assert_clause_count(r#"MERGE (n:Person {id: 1}) RETURN n"#, 2);
}

#[test]
fn create_match_where_return_pipeline() {
    assert_clause_count(r#"CREATE (n:Person {name: "A"}) RETURN n.name"#, 2);
}

#[test]
fn trailing_semicolon_is_ignored() {
    let s = parse("CREATE (n) RETURN n;").unwrap();
    assert_eq!(s.clauses.len(), 2);
}

#[test]
fn keyword_case_insensitive() {
    let s = parse("create (n:Person) return n.name").unwrap();
    assert_eq!(s.clauses.len(), 2);
}

#[test]
fn keyword_mixed_case() {
    let s = parse("CrEaTe (n:Person) ReTuRn n.name").unwrap();
    assert_eq!(s.clauses.len(), 2);
}

#[test]
fn single_line_comment() {
    let s = parse("// this is a comment\nCREATE (n) RETURN n").unwrap();
    assert_eq!(s.clauses.len(), 2);
}
