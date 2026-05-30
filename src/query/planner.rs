use crate::errors::DbError;
use crate::query::ast::{
    BinaryOp, Direction, Expression, MatchClause, NodePattern, PathPattern, Pattern,
    RelationshipLength, ReturnItem, SetClause, Statement,
};
use crate::query::utils::expression_to_literal;
use crate::value::Value;

#[derive(Debug, Clone)]
pub enum QueryPlan {
    CreateNode {
        node: NodePattern,
    },
    MergeNode {
        node: NodePattern,
    },
    CreateRelationship {
        src: NodePattern,
        dst: NodePattern,
        rel_type: String,
        properties: Vec<(String, Value)>,
    },
    ScanNodes {
        variable: String,
        label: Option<String>,
        filter: Option<Expression>,
    },
    TraverseEdges {
        from_var: String,
        edge_type: Option<String>,
        direction: Direction,
        to_var: String,
        to_label: Option<String>,
        hops: Option<RelationshipLength>,
    },
    Filter {
        condition: Expression,
    },
    Return {
        items: Vec<ReturnItem>,
    },
    DeleteEntity {
        variable: String,
    },
    SetProperty {
        variable: String,
        key: String,
        value: Value,
    },
    Sequence(Vec<QueryPlan>),
}

/// Converts a parsed `Statement` AST into an executable `QueryPlan`.
pub fn plan(stmt: Statement) -> Result<QueryPlan, DbError> {
    match stmt {
        Statement::Create(input) => plan_create(input),
        Statement::Merge(input) => plan_merge(input),
        Statement::Match(match_clause) => plan_match(*match_clause),
        Statement::Delete(var) => Ok(QueryPlan::DeleteEntity { variable: var }),
        Statement::Set(set_clause) => plan_set(*set_clause),
    }
}

/// Converts a CREATE node pattern into a `CreateNode` plan step.
fn plan_create(input: Pattern) -> Result<QueryPlan, DbError> {
    match input {
        Pattern::Node(node) => Ok(QueryPlan::CreateNode { node }),
        Pattern::Path(PathPattern { start, segments }) => {
            if segments.len() != 1 {
                return Err(DbError::QueryError(
                    "CREATE currently supports exactly one relationship segment".to_string(),
                ));
            }
            let first = &segments[0];

            let rel = first.relationship.clone();
            let rel_type = rel.rel_type.ok_or(DbError::QueryError(
                "CREATE requires a label (e.g., CREATE (n:Person {...}))".to_string(),
            ))?;

            let mut properties = Vec::new();

            for (key, expr) in &rel.properties {
                let value = expression_to_literal(expr)?;
                properties.push((key.clone(), value));
            }

            Ok(QueryPlan::CreateRelationship {
                src: start,
                dst: first.node.clone(),
                rel_type,
                properties,
            })
        }
    }
}

fn plan_merge(input: Pattern) -> Result<QueryPlan, DbError> {
    match input {
        Pattern::Node(node) => Ok(QueryPlan::MergeNode { node }),
        Pattern::Path(_) => Err(DbError::QueryError(
            "MERGE currently supports only single node patterns".to_string(),
        )),
    }
}

/// Converts a MATCH clause into a sequence of `ScanNodes`, `TraverseEdges`,
/// `Filter`, and `Return` plan steps.
fn plan_match(clause: MatchClause) -> Result<QueryPlan, DbError> {
    let mut steps = Vec::new();

    match clause.pattern {
        Pattern::Node(ref node) => {
            let mut filter_conditions = Vec::new();

            for (key, value_expr) in &node.properties {
                filter_conditions.push(Expression::BinaryOp {
                    left: Box::new(Expression::Property {
                        variable: node.variable.clone().unwrap(),
                        property: key.clone(),
                    }),
                    op: BinaryOp::Eq,
                    right: Box::new(value_expr.clone()),
                });
            }

            let combined_filter = merge_conditions(
                filter_conditions,
                clause.where_clause.clone().map(|w| w.condition),
            );

            steps.push(QueryPlan::ScanNodes {
                variable: node.variable.clone().unwrap(),
                label: node.label.clone(),
                filter: combined_filter,
            });
        }
        Pattern::Path(PathPattern { start, segments }) => {
            let mut first_filter = Vec::new();
            for (key, value_expr) in &start.properties {
                first_filter.push(Expression::BinaryOp {
                    left: Box::new(Expression::Property {
                        variable: start.variable.clone().unwrap(),
                        property: key.clone(),
                    }),
                    op: BinaryOp::Eq,
                    right: Box::new(value_expr.clone()),
                });
            }

            steps.push(QueryPlan::ScanNodes {
                variable: start.variable.clone().unwrap(),
                label: start.label.clone(),
                filter: if first_filter.is_empty() {
                    None
                } else {
                    merge_conditions(first_filter, None)
                },
            });

            let mut from_var = start.variable.clone().unwrap();

            for seg in &segments {
                let to_variable = seg.node.variable.clone().unwrap();

                steps.push(QueryPlan::TraverseEdges {
                    from_var: from_var.clone(),
                    edge_type: seg.relationship.rel_type.clone(),
                    direction: seg.relationship.direction.clone(),
                    to_var: to_variable.clone(),
                    to_label: seg.node.label.clone(),
                    hops: seg.relationship.hops.clone(),
                });

                let mut node_filter = Vec::new();

                for (key, value_expr) in &seg.node.properties {
                    node_filter.push(Expression::BinaryOp {
                        left: Box::new(Expression::Property {
                            variable: to_variable.clone(),
                            property: key.clone(),
                        }),
                        op: BinaryOp::Eq,
                        right: Box::new(value_expr.clone()),
                    });
                }

                if !node_filter.is_empty() {
                    steps.push(QueryPlan::Filter {
                        condition: merge_conditions(node_filter, None)
                            .ok_or(DbError::QueryError("Empty filter".to_string()))?,
                    });
                }

                from_var = to_variable;
            }

            if let Some(where_clause) = clause.where_clause {
                steps.push(QueryPlan::Filter {
                    condition: where_clause.condition,
                });
            }
        }
    }

    steps.push(QueryPlan::Return {
        items: clause.return_clause.items,
    });

    if steps.len() == 1 {
        Ok(steps.into_iter().next().unwrap())
    } else {
        Ok(QueryPlan::Sequence(steps))
    }
}

/// Converts a SET clause into a `SetProperty` plan step.
fn plan_set(clause: SetClause) -> Result<QueryPlan, DbError> {
    let value = expression_to_literal(&clause.value)?;
    Ok(QueryPlan::SetProperty {
        variable: clause.variable,
        key: clause.property,
        value,
    })
}

/// Merges inline property equality conditions with an optional WHERE clause
/// into a single AND-chained expression, if any conditions exist.
fn merge_conditions(
    prop_conditions: Vec<Expression>,
    where_condition: Option<Expression>,
) -> Option<Expression> {
    match (prop_conditions.is_empty(), where_condition) {
        (true, None) => None,
        (true, Some(w)) => Some(w),
        (false, None) => Some(and_chain(prop_conditions)),
        (false, Some(w)) => {
            let mut all = prop_conditions;
            all.push(w);
            Some(and_chain(all))
        }
    }
}

/// Chains a list of expressions with AND binary operators.
fn and_chain(conditions: Vec<Expression>) -> Expression {
    conditions
        .into_iter()
        .reduce(|left, right| Expression::BinaryOp {
            left: Box::new(left),
            op: BinaryOp::And,
            right: Box::new(right),
        })
        .unwrap()
}
