use crate::query::ast::*;
use crate::query::parser::parse;

fn clauses(input: &str) -> Vec<Clause> {
    parse(input)
        .unwrap_or_else(|e| panic!("parse failed for `{input}`: {e}"))
        .clauses
}

#[test]
fn with_projects_alias() {
    let c = clauses(r#"MATCH (n:Person) WITH n AS m RETURN m"#);
    assert!(matches!(c[0], Clause::Match(_)));
    match &c[1] {
        Clause::With(w) => {
            assert_eq!(w.items.len(), 1);
            assert_eq!(w.items[0].alias.as_deref(), Some("m"));
        }
        other => panic!("expected With, got {other:?}"),
    }
    assert!(matches!(c[2], Clause::Return(_)));
}

#[test]
fn optional_match_parses() {
    let c = clauses(r#"MATCH (n:Person) OPTIONAL MATCH (n)-[:KNOWS]->(m) RETURN m"#);
    assert!(matches!(c[0], Clause::Match(_)));
    assert!(matches!(c[1], Clause::OptionalMatch(_)));
    assert!(matches!(c[2], Clause::Return(_)));
}

#[test]
fn remove_label_and_property() {
    let c = clauses(r#"MATCH (n:Person) REMOVE n:Employee, n.age"#);
    match &c[1] {
        Clause::Remove(r) => {
            assert_eq!(r.items.len(), 2);
            assert_eq!(r.items[0].label.as_deref(), Some("Employee"));
            assert_eq!(r.items[1].property.as_deref(), Some("age"));
        }
        other => panic!("expected Remove, got {other:?}"),
    }
}

#[test]
fn merge_on_create_and_match_set() {
    let c = clauses(
        r#"MERGE (n:Person {email: "a@b.com"}) ON CREATE SET n.created = true ON MATCH SET n.seen = true"#,
    );
    assert!(matches!(c[0], Clause::Merge(_)));
    match &c[1] {
        Clause::OnCreate(s) => assert_eq!(s.property, "created"),
        other => panic!("expected OnCreate, got {other:?}"),
    }
    match &c[2] {
        Clause::OnMatch(s) => assert_eq!(s.property, "seen"),
        other => panic!("expected OnMatch, got {other:?}"),
    }
}

#[test]
fn merge_relationship_parses() {
    let c = clauses(r#"MERGE (a:Person)-[:KNOWS]->(b:Person)"#);
    match &c[0] {
        Clause::Merge(Pattern::Path(path)) => {
            assert_eq!(path.segments.len(), 1);
        }
        other => panic!("expected Merge(Path), got {other:?}"),
    }
}

#[test]
fn params_parse_in_where_and_set() {
    let c = clauses(r#"MATCH (n:Person) WHERE n.age = $age RETURN n"#);
    match &c[1] {
        Clause::Where(Expression::BinaryOp { right, .. }) => {
            assert!(matches!(&**right, Expression::Param(p) if p == "age"));
        }
        other => panic!("expected Where, got {other:?}"),
    }
    let c = clauses(r#"MATCH (n:Person) SET n.age = $age"#);
    match &c[1] {
        Clause::Set(s) => assert!(matches!(&s.value, Expression::Param(p) if p == "age")),
        other => panic!("expected Set, got {other:?}"),
    }
}

#[test]
fn count_parses_in_return() {
    let c = clauses(r#"MATCH (n:Person) RETURN COUNT(*)"#);
    match &c[1] {
        Clause::Return(r) => {
            assert!(matches!(
                &r.items[0].expression,
                Expression::Count(t) if matches!(&**t, CountTarget::Star)
            ));
        }
        other => panic!("expected Return, got {other:?}"),
    }
    let c = clauses(r#"MATCH (n:Person) RETURN COUNT(n.age)"#);
    match &c[1] {
        Clause::Return(r) => {
            assert!(matches!(
                &r.items[0].expression,
                Expression::Count(t) if matches!(&**t, CountTarget::Expr(_))
            ));
        }
        other => panic!("expected Return, got {other:?}"),
    }
}

#[test]
fn extended_aggregates_parse_in_return() {
    for query in [
        r#"MATCH (n:Person) RETURN SUM(n.age)"#,
        r#"MATCH (n:Person) RETURN AVG(n.age)"#,
        r#"MATCH (n:Person) RETURN MIN(n.age)"#,
        r#"MATCH (n:Person) RETURN MAX(n.age)"#,
    ] {
        let c = clauses(query);
        match &c[1] {
            Clause::Return(r) => match &r.items[0].expression {
                Expression::Sum(_) if query.contains("SUM") => {}
                Expression::Avg(_) if query.contains("AVG") => {}
                Expression::Min(_) if query.contains("MIN") => {}
                Expression::Max(_) if query.contains("MAX") => {}
                other => panic!("wrong aggregate for `{query}`, got {other:?}"),
            },
            other => panic!("expected Return, got {other:?}"),
        }
    }
    // Lowercase spellings lex to the same aggregate functions.
    let c = clauses(r#"MATCH (n:Person) RETURN sum(n.age), avg(n.age), min(n.age), max(n.age)"#);
    match &c[1] {
        Clause::Return(r) => {
            assert!(matches!(&r.items[0].expression, Expression::Sum(_)));
            assert!(matches!(&r.items[1].expression, Expression::Avg(_)));
            assert!(matches!(&r.items[2].expression, Expression::Min(_)));
            assert!(matches!(&r.items[3].expression, Expression::Max(_)));
        }
        other => panic!("expected Return, got {other:?}"),
    }
    // Aggregate keywords still work as property names.
    let c = clauses(r#"MATCH (n:Person) SET n.sum = 1"#);
    match &c[1] {
        Clause::Set(s) => assert_eq!(s.property, "sum"),
        other => panic!("expected Set, got {other:?}"),
    }
    // Star is only valid inside COUNT(...).
    assert!(parse(r#"MATCH (n) RETURN SUM(*)"#).is_err());
}

#[test]
fn multi_label_node_pattern() {
    let c = clauses(r#"CREATE (n:Person:Employee {name: "A"})"#);
    match &c[0] {
        Clause::Create(Pattern::Node(node)) => {
            assert_eq!(node.label.as_deref(), Some("Person"));
            assert_eq!(node.extra_labels, vec!["Employee".to_string()]);
            assert_eq!(node.all_labels().len(), 2);
        }
        other => panic!("expected Create(Node), got {other:?}"),
    }
}

#[test]
fn null_and_negative_literals() {
    let c = clauses(r#"CREATE (n:Person {nick: NULL, score: -5})"#);
    match &c[0] {
        Clause::Create(Pattern::Node(node)) => {
            assert_eq!(node.properties.get("nick"), Some(&Expression::Null));
            assert_eq!(node.properties.get("score"), Some(&Expression::Integer(-5)));
        }
        other => panic!("expected Create(Node), got {other:?}"),
    }
}

#[test]
fn var_length_bounds_parse() {
    let c = clauses(r#"MATCH (a)-[:KNOWS*1..3]->(b) RETURN b"#);
    match &c[0] {
        Clause::Match(m) => match &m.pattern {
            Pattern::Path(p) => {
                let hops = p.segments[0].relationship.hops.as_ref().unwrap();
                assert_eq!(hops.min_hops, Some(1));
                assert_eq!(hops.max_hops, Some(3));
            }
            other => panic!("expected path, got {other:?}"),
        },
        other => panic!("expected Match, got {other:?}"),
    }
}
