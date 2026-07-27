use crate::query::ast::*;
use crate::query::parser::parse;
use crate::query::planner::{NodeIndexHint, QueryPlan, plan};
use crate::value::Value;

fn plan_query(input: &str) -> QueryPlan {
    let stmt = parse(input).unwrap_or_else(|e| panic!("parse failed for `{input}`: {e}"));
    plan(stmt).unwrap_or_else(|e| panic!("plan failed for `{input}`: {e}"))
}

fn plan_query_err(input: &str) -> String {
    let stmt = parse(input).unwrap_or_else(|e| panic!("parse failed for `{input}`: {e}"));
    plan(stmt).unwrap_err().to_string()
}

fn assert_plan_step(plan: &QueryPlan, expected: &str) {
    match plan {
        QueryPlan::Sequence(steps) => {
            assert!(
                !steps.is_empty(),
                "expected at least one step, got empty sequence"
            );
            let actual = format!("{:#?}", steps.last().unwrap());
            assert!(
                actual.contains(expected),
                "expected plan step containing `{expected}`, got:\n{actual}"
            );
        }
        other => {
            let actual = format!("{other:#?}");
            assert!(
                actual.contains(expected),
                "expected plan step containing `{expected}`, got:\n{actual}"
            );
        }
    }
}

// ---------- CREATE ----------

#[test]
fn plan_create_node_with_label() {
    let p = plan_query(r#"CREATE (n:Person)"#);
    assert_plan_step(&p, "CreateNode");
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::CreateNode { variable, node } => {
                assert_eq!(variable.as_deref(), Some("n"));
                assert_eq!(node.label.as_deref(), Some("Person"));
            }
            other => panic!("expected CreateNode, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_create_node_with_properties() {
    let p = plan_query(r#"CREATE (n:Person {name: "Alice", age: 30})"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::CreateNode { node, .. } => {
                assert_eq!(node.properties.len(), 2);
                assert_eq!(
                    *node.properties.get("name").unwrap(),
                    Expression::String("Alice".to_string())
                );
                assert_eq!(
                    *node.properties.get("age").unwrap(),
                    Expression::Integer(30)
                );
            }
            other => panic!("expected CreateNode, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_create_relationship_path() {
    let p = plan_query(r#"CREATE (a:Person)-[:KNOWS]->(b:Person)"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::CreateRelationship {
                src, dst, rel_type, ..
            } => {
                assert_eq!(rel_type, "KNOWS");
                assert_eq!(src.label.as_deref(), Some("Person"));
                assert_eq!(dst.label.as_deref(), Some("Person"));
            }
            other => panic!("expected CreateRelationship, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_create_relationship_with_properties() {
    let p = plan_query(r#"CREATE (a:Person)-[:KNOWS {since: 2020}]->(b:Person)"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::CreateRelationship { properties, .. } => {
                assert_eq!(properties.len(), 1);
                assert_eq!(properties[0].0, "since");
                assert_eq!(properties[0].1, Expression::Integer(2020));
            }
            other => panic!("expected CreateRelationship, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_create_multi_hop_rejects() {
    let err = plan_query_err(r#"CREATE (a)-[:KNOWS]->(b)-[:LIKES]->(c)"#);
    assert!(
        err.contains("exactly one relationship segment"),
        "unexpected error: {err}"
    );
}

// ---------- MATCH ----------

#[test]
fn plan_match_node_with_label() {
    let p = plan_query(r#"MATCH (n:Person) RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::ScanNodes {
                variable,
                label,
                index_hint,
                ..
            } => {
                assert_eq!(variable, "n");
                assert_eq!(label.as_deref(), Some("Person"));
                assert!(matches!(index_hint, NodeIndexHint::Label { label } if label == "Person"));
            }
            other => panic!("expected ScanNodes, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_node_without_label() {
    let p = plan_query(r#"MATCH (n) RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::ScanNodes {
                label, index_hint, ..
            } => {
                assert!(label.is_none());
                assert!(matches!(index_hint, NodeIndexHint::FullScan));
            }
            other => panic!("expected ScanNodes, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_path_traversal() {
    let p = plan_query(r#"MATCH (a:Person)-[r:KNOWS]->(b:Person) RETURN a, r, b"#);
    match p {
        QueryPlan::Sequence(steps) => {
            assert!(
                matches!(&steps[0], QueryPlan::ScanNodes { .. }),
                "first step should be ScanNodes"
            );
            assert!(
                matches!(&steps[1], QueryPlan::TraverseEdges { .. }),
                "second step should be TraverseEdges"
            );
            match &steps[1] {
                QueryPlan::TraverseEdges {
                    from_var,
                    edge_type,
                    direction,
                    to_var,
                    edge_var,
                    ..
                } => {
                    assert_eq!(from_var, "a");
                    assert_eq!(edge_type.as_deref(), Some("KNOWS"));
                    assert!(matches!(direction, Direction::Outgoing));
                    assert_eq!(to_var, "b");
                    assert_eq!(edge_var.as_deref(), Some("r"));
                }
                other => panic!("expected TraverseEdges, got {other:?}"),
            }
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_incoming_direction() {
    let p = plan_query(r#"MATCH (a)<-[r:KNOWS]-(b) RETURN a"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[1] {
            QueryPlan::TraverseEdges { direction, .. } => {
                assert!(matches!(direction, Direction::Incoming));
            }
            other => panic!("expected TraverseEdges, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_undirected_direction() {
    let p = plan_query(r#"MATCH (a)-[r:KNOWS]-(b) RETURN a"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[1] {
            QueryPlan::TraverseEdges { direction, .. } => {
                assert!(matches!(direction, Direction::Undirected));
            }
            other => panic!("expected TraverseEdges, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_node_requires_variable() {
    let err = plan_query_err(r#"MATCH (:Person) RETURN 1"#);
    assert!(
        err.contains("requires a variable"),
        "unexpected error: {err}"
    );
}

// ---------- WHERE ----------

#[test]
fn plan_where_filter() {
    let p = plan_query(r#"MATCH (n:Person) WHERE n.age >= 30 RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => {
            assert!(
                matches!(&steps[0], QueryPlan::ScanNodes { .. }),
                "first step should be ScanNodes"
            );
            assert!(
                matches!(&steps[1], QueryPlan::Filter { .. }),
                "second step should be Filter"
            );
            match &steps[1] {
                QueryPlan::Filter { condition } => match condition {
                    Expression::BinaryOp { op, .. } => {
                        assert!(matches!(op, BinaryOp::Gte));
                    }
                    other => panic!("expected BinaryOp, got {other:?}"),
                },
                other => panic!("expected Filter, got {other:?}"),
            }
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_where_references_unknown_variable() {
    let err = plan_query_err(r#"MATCH (n:Person) WHERE m.age > 20 RETURN n"#);
    assert!(err.contains("unknown variable"), "unexpected error: {err}");
}

// ---------- SET ----------

#[test]
fn plan_set_property() {
    let p = plan_query(r#"MATCH (n:Person) SET n.name = "Bob" RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => {
            let set_step = steps
                .iter()
                .find(|s| matches!(s, QueryPlan::SetProperty { .. }));
            assert!(set_step.is_some(), "expected SetProperty step");
            match set_step.unwrap() {
                QueryPlan::SetProperty { variable, key, .. } => {
                    assert_eq!(variable, "n");
                    assert_eq!(key, "name");
                }
                other => panic!("expected SetProperty, got {other:?}"),
            }
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_set_references_unknown_variable() {
    let err = plan_query_err(r#"MATCH (n:Person) SET m.name = "Bob" RETURN n"#);
    assert!(err.contains("unknown variable"), "unexpected error: {err}");
}

// ---------- DELETE ----------

#[test]
fn plan_delete_node() {
    let p = plan_query(r#"MATCH (n:Person) DELETE n"#);
    match p {
        QueryPlan::Sequence(steps) => {
            let del_step = steps.iter().find(|s| matches!(s, QueryPlan::Delete { .. }));
            assert!(del_step.is_some(), "expected Delete step");
            match del_step.unwrap() {
                QueryPlan::Delete { variables, detach } => {
                    assert_eq!(variables, &vec!["n".to_string()]);
                    assert!(!detach);
                }
                other => panic!("expected Delete, got {other:?}"),
            }
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_detach_delete() {
    let p = plan_query(r#"MATCH (n:Person) DETACH DELETE n"#);
    match p {
        QueryPlan::Sequence(steps) => {
            let del_step = steps.iter().find(|s| matches!(s, QueryPlan::Delete { .. }));
            match del_step.unwrap() {
                QueryPlan::Delete { detach, .. } => {
                    assert!(detach);
                }
                other => panic!("expected Delete, got {other:?}"),
            }
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_delete_multiple_variables() {
    let p = plan_query(r#"MATCH (n:Person)-[r:KNOWS]->(m:Person) DELETE r, m"#);
    match p {
        QueryPlan::Sequence(steps) => {
            let del_step = steps.iter().find(|s| matches!(s, QueryPlan::Delete { .. }));
            match del_step.unwrap() {
                QueryPlan::Delete { variables, .. } => {
                    assert!(variables.contains(&"r".to_string()));
                    assert!(variables.contains(&"m".to_string()));
                }
                other => panic!("expected Delete, got {other:?}"),
            }
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_delete_references_unknown_variable() {
    let err = plan_query_err(r#"MATCH (n:Person) DELETE m"#);
    assert!(err.contains("unknown variable"), "unexpected error: {err}");
}

// ---------- MERGE ----------

#[test]
fn plan_merge_node() {
    let p = plan_query(r#"MERGE (n:Person {name: "Alice"})"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::MergeNode { variable, node } => {
                assert_eq!(variable.as_deref(), Some("n"));
                assert_eq!(node.label.as_deref(), Some("Person"));
                assert_eq!(node.properties.len(), 1);
            }
            other => panic!("expected MergeNode, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_merge_path_rejects() {
    let err = plan_query_err(r#"MERGE (a)-[:KNOWS]->(b)"#);
    assert!(
        err.contains("only single node patterns"),
        "unexpected error: {err}"
    );
}

// ---------- RETURN ----------

#[test]
fn plan_return_variable() {
    let p = plan_query(r#"MATCH (n:Person) RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match steps.last().unwrap() {
            QueryPlan::Return(ret) => {
                assert_eq!(ret.items.len(), 1);
                assert!(matches!(&ret.items[0].expression, Expression::Variable(v) if v == "n"));
            }
            other => panic!("expected Return, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_return_property() {
    let p = plan_query(r#"MATCH (n:Person) RETURN n.name AS name"#);
    match p {
        QueryPlan::Sequence(steps) => match steps.last().unwrap() {
            QueryPlan::Return(ret) => {
                assert_eq!(ret.items.len(), 1);
                assert_eq!(ret.items[0].alias.as_deref(), Some("name"));
                assert!(
                    matches!(&ret.items[0].expression, Expression::Property { variable, property } if variable == "n" && property == "name")
                );
            }
            other => panic!("expected Return, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_return_with_order_by() {
    let p = plan_query(r#"MATCH (n:Person) RETURN n.name ORDER BY n.age DESC"#);
    match p {
        QueryPlan::Sequence(steps) => match steps.last().unwrap() {
            QueryPlan::Return(ret) => {
                assert_eq!(ret.order_by.len(), 1);
                assert!(ret.order_by[0].descending);
            }
            other => panic!("expected Return, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_return_with_skip_limit() {
    let p = plan_query(r#"MATCH (n:Person) RETURN n.name SKIP 5 LIMIT 10"#);
    match p {
        QueryPlan::Sequence(steps) => match steps.last().unwrap() {
            QueryPlan::Return(ret) => {
                assert_eq!(ret.skip, Some(5));
                assert_eq!(ret.limit, Some(10));
            }
            other => panic!("expected Return, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_return_references_unknown_variable() {
    let err = plan_query_err(r#"MATCH (n:Person) RETURN m.name"#);
    assert!(err.contains("unknown variable"), "unexpected error: {err}");
}

// ---------- INDEX HINTS ----------

#[test]
fn plan_index_hint_label_only() {
    let p = plan_query(r#"MATCH (n:Person) RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::ScanNodes { index_hint, .. } => {
                assert!(
                    matches!(index_hint, NodeIndexHint::Label { label } if label == "Person"),
                    "expected Label hint, got {index_hint:?}"
                );
            }
            other => panic!("expected ScanNodes, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_index_hint_label_and_property_equality() {
    let p = plan_query(r#"MATCH (n:Person {name: "Alice"}) RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::ScanNodes { index_hint, .. } => {
                assert!(
                    matches!(index_hint, NodeIndexHint::LabelAndProperty { label, key, value }
                        if label == "Person" && key == "name" && *value == Value::String("Alice".to_string())),
                    "expected LabelAndProperty hint, got {index_hint:?}"
                );
            }
            other => panic!("expected ScanNodes, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_index_hint_property_only_no_label() {
    let p = plan_query(r#"MATCH (n {name: "Alice"}) RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::ScanNodes { index_hint, .. } => {
                assert!(
                    matches!(index_hint, NodeIndexHint::Property { key, value }
                        if key == "name" && *value == Value::String("Alice".to_string())),
                    "expected Property hint, got {index_hint:?}"
                );
            }
            other => panic!("expected ScanNodes, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_index_hint_full_scan() {
    let p = plan_query(r#"MATCH (n) RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::ScanNodes { index_hint, .. } => {
                assert!(
                    matches!(index_hint, NodeIndexHint::FullScan),
                    "expected FullScan hint, got {index_hint:?}"
                );
            }
            other => panic!("expected ScanNodes, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

// ---------- VARIABLE-LENGTH TRAVERSAL ----------

#[test]
fn plan_variable_length_traversal_rejects() {
    let err = plan_query_err(r#"MATCH (a)-[*1..3]->(b) RETURN a, b"#);
    assert!(err.contains("variable-length"), "unexpected error: {err}");
}

#[test]
fn plan_unbounded_traversal_rejects() {
    let err = plan_query_err(r#"MATCH (a)-[*]->(b) RETURN a, b"#);
    assert!(err.contains("variable-length"), "unexpected error: {err}");
}

// ---------- COMPLEX PIPELINES ----------

#[test]
fn plan_create_match_set_return_pipeline() {
    let p = plan_query(r#"CREATE (n:Person {name: "Alice"}) RETURN n.name"#);
    match p {
        QueryPlan::Sequence(steps) => {
            assert!(
                matches!(&steps[0], QueryPlan::CreateNode { .. }),
                "first step should be CreateNode"
            );
            assert!(
                matches!(steps.last().unwrap(), QueryPlan::Return(_)),
                "last step should be Return"
            );
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_where_set_delete_return() {
    let p = plan_query(
        r#"MATCH (n:Person) WHERE n.age < 18 SET n.minor = true DELETE n RETURN n.name"#,
    );
    match p {
        QueryPlan::Sequence(steps) => {
            assert!(
                steps.len() >= 4,
                "expected at least 4 steps, got {}",
                steps.len()
            );
            assert!(matches!(&steps[0], QueryPlan::ScanNodes { .. }));
            assert!(matches!(&steps[1], QueryPlan::Filter { .. }));
            assert!(matches!(&steps[2], QueryPlan::SetProperty { .. }));
            assert!(matches!(&steps[3], QueryPlan::Delete { .. }));
            assert!(matches!(steps.last().unwrap(), QueryPlan::Return(_)));
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_traverse_filter_return() {
    let p = plan_query(
        r#"MATCH (a:Person)-[r:KNOWS]->(b:Person) WHERE b.age > 25 RETURN a.name, b.name"#,
    );
    match p {
        QueryPlan::Sequence(steps) => {
            assert!(steps.len() >= 3);
            assert!(matches!(&steps[0], QueryPlan::ScanNodes { .. }));
            assert!(matches!(&steps[1], QueryPlan::TraverseEdges { .. }));
            assert!(matches!(&steps[2], QueryPlan::Filter { .. }));
            assert!(matches!(steps.last().unwrap(), QueryPlan::Return(_)));
        }
        other => panic!("expected Sequence, got {other:?}"),
    }
}

// ---------- IS_READ_ONLY ----------

#[test]
fn read_only_plans_are_detected() {
    let read_only = [
        r#"MATCH (n:Person) RETURN n"#,
        r#"MATCH (n:Person) WHERE n.age > 20 RETURN n"#,
        r#"MATCH (a)-[:KNOWS]->(b) RETURN a, b"#,
    ];
    for q in &read_only {
        let p = plan_query(q);
        assert!(p.is_read_only(), "expected read-only for `{q}`");
    }

    let mutating = [
        r#"CREATE (n:Person)"#,
        r#"MERGE (n:Person {name: "Alice"})"#,
        r#"MATCH (n:Person) SET n.name = "Bob""#,
        r#"MATCH (n:Person) DELETE n"#,
    ];
    for q in &mutating {
        let p = plan_query(q);
        assert!(!p.is_read_only(), "expected mutable for `{q}`");
    }
}

// ---------- EDGE CASES ----------

#[test]
fn plan_create_node_without_label() {
    let p = plan_query(r#"CREATE (n)"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::CreateNode { node, .. } => {
                assert!(node.label.is_none());
            }
            other => panic!("expected CreateNode, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_create_node_without_variable() {
    let p = plan_query(r#"CREATE (:Person)"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::CreateNode { variable, .. } => {
                assert!(variable.is_none());
            }
            other => panic!("expected CreateNode, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_multiple_return_items() {
    let p = plan_query(r#"MATCH (n:Person) RETURN n.name, n.age, n.email"#);
    match p {
        QueryPlan::Sequence(steps) => match steps.last().unwrap() {
            QueryPlan::Return(ret) => {
                assert_eq!(ret.items.len(), 3);
            }
            other => panic!("expected Return, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_return_with_asc_order() {
    let p = plan_query(r#"MATCH (n:Person) RETURN n.name ORDER BY n.age ASC"#);
    match p {
        QueryPlan::Sequence(steps) => match steps.last().unwrap() {
            QueryPlan::Return(ret) => {
                assert!(!ret.order_by[0].descending);
            }
            other => panic!("expected Return, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_with_equality_filter_generates_index_hint() {
    let p = plan_query(r#"MATCH (n:Person {name: "Alice"}) RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::ScanNodes {
                index_hint, filter, ..
            } => {
                assert!(
                    matches!(index_hint, NodeIndexHint::LabelAndProperty { .. }),
                    "expected LabelAndProperty hint for equality filter"
                );
                assert!(filter.is_some(), "expected filter to be set");
            }
            other => panic!("expected ScanNodes, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}

#[test]
fn plan_match_with_inequality_does_not_generate_property_hint() {
    let p = plan_query(r#"MATCH (n:Person) WHERE n.age > 20 RETURN n"#);
    match p {
        QueryPlan::Sequence(steps) => match &steps[0] {
            QueryPlan::ScanNodes { index_hint, .. } => {
                assert!(
                    matches!(index_hint, NodeIndexHint::Label { .. }),
                    "expected Label-only hint for non-equality filter, got {index_hint:?}"
                );
            }
            other => panic!("expected ScanNodes, got {other:?}"),
        },
        other => panic!("expected Sequence, got {other:?}"),
    }
}
