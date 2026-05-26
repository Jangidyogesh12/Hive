use crate::errors::DbError;
use crate::query::ast::{
    BinaryOp, Direction, Expression, MatchClause, NodePattern, Pattern, ReturnItem, SetClause,
    Statement,
};
use crate::query::utils::expression_to_literal;
use crate::value::Value;

#[derive(Debug, Clone)]
pub enum QueryPlan {
    CreateNode {
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
        Statement::Match(match_clause) => plan_match(*match_clause),
        Statement::Delete(var) => Ok(QueryPlan::DeleteEntity { variable: var }),
        Statement::Set(set_clause) => plan_set(*set_clause),
    }
}

/// Converts a CREATE node pattern into a `CreateNode` plan step.
fn plan_create(input: Pattern) -> Result<QueryPlan, DbError> {
    match input {
        Pattern::Node(node) => Ok(QueryPlan::CreateNode { node }),
        Pattern::Edge(left, rel, right) => {
            let rel_type = rel.rel_type.ok_or(DbError::QueryError(
                "CREATE requires a label (e.g., CREATE (n:Person {...}))".to_string(),
            ))?;

            let mut properties = Vec::new();

            for (key, expr) in &rel.properties {
                let value = expression_to_literal(expr)?;
                properties.push((key.clone(), value));
            }

            Ok(QueryPlan::CreateRelationship {
                src: *left,
                dst: *right,
                rel_type,
                properties,
            })
        }
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
        Pattern::Edge(ref first, ref rel, ref second) => {
            let mut first_filter = Vec::new();
            for (key, value_expr) in &first.properties {
                first_filter.push(Expression::BinaryOp {
                    left: Box::new(Expression::Property {
                        variable: first.variable.clone().unwrap(),
                        property: key.clone(),
                    }),
                    op: BinaryOp::Eq,
                    right: Box::new(value_expr.clone()),
                });
            }

            steps.push(QueryPlan::ScanNodes {
                variable: first.variable.clone().unwrap(),
                label: first.label.clone(),
                filter: if first_filter.is_empty() {
                    None
                } else {
                    merge_conditions(first_filter, None)
                },
            });

            steps.push(QueryPlan::TraverseEdges {
                from_var: first.variable.clone().unwrap(),
                edge_type: rel.rel_type.clone(),
                direction: rel.direction.clone(),
                to_var: second.variable.clone().unwrap(),
                to_label: second.label.clone(),
            });

            let mut second_filter = Vec::new();
            for (key, value_expr) in &second.properties {
                second_filter.push(Expression::BinaryOp {
                    left: Box::new(Expression::Property {
                        variable: second.variable.clone().unwrap(),
                        property: key.clone(),
                    }),
                    op: BinaryOp::Eq,
                    right: Box::new(value_expr.clone()),
                });
            }

            if !second_filter.is_empty() {
                steps.push(QueryPlan::Filter {
                    condition: merge_conditions(second_filter, None)
                        .ok_or(DbError::QueryError("Empty filter".to_string()))?,
                });
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
