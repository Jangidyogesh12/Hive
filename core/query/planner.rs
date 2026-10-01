use std::collections::HashSet;

use crate::errors::DbError;
use crate::query::ast::{
    BinaryOp, Clause, Direction, Expression, MatchClause, NodePattern, PathPattern, Pattern,
    RelationshipLength, RemoveClause, ReturnClause, SetClause, Statement,
};
use crate::query::utils::expression_to_literal;
use crate::value::Value;

#[derive(Debug, Clone)]
pub enum QueryPlan {
    CreateNode {
        variable: Option<String>,
        node: NodePattern,
    },
    MergeNode {
        variable: Option<String>,
        node: NodePattern,
        on_create: Vec<SetClause>,
        on_match: Vec<SetClause>,
    },
    MergeRelationship {
        src: NodePattern,
        dst: NodePattern,
        rel_type: String,
        rel_var: Option<String>,
        properties: Vec<(String, Expression)>,
        on_create: Vec<SetClause>,
        on_match: Vec<SetClause>,
    },
    CreateRelationship {
        src: NodePattern,
        dst: NodePattern,
        rel_type: String,
        properties: Vec<(String, Expression)>,
    },
    ScanNodes {
        variable: String,
        label: Option<String>,
        extra_labels: Vec<String>,
        filter: Option<Expression>,
        index_hint: NodeIndexHint,
        optional: bool,
    },
    TraverseEdges {
        from_var: String,
        edge_type: Option<String>,
        direction: Direction,
        to_var: String,
        to_label: Option<String>,
        to_extra_labels: Vec<String>,
        hops: Option<RelationshipLength>,
        edge_var: Option<String>,
        edge_filter: Option<Expression>,
        optional: bool,
    },
    Filter {
        condition: Expression,
    },
    With(ReturnClause),
    Return(ReturnClause),
    Delete {
        variables: Vec<String>,
        detach: bool,
    },
    Remove(RemoveClause),
    SetProperty {
        variable: String,
        key: String,
        value: Expression,
    },
    Sequence(Vec<QueryPlan>),
}

#[derive(Debug, Clone)]
pub enum NodeIndexHint {
    FullScan,
    Label {
        label: String,
    },
    Property {
        key: String,
        value: Value,
    },
    LabelAndProperty {
        label: String,
        key: String,
        value: Value,
    },
}

impl QueryPlan {
    /// Returns `true` if this plan step performs no mutations (no CREATE, MERGE, SET, DELETE, REMOVE).
    pub fn is_read_only(&self) -> bool {
        match self {
            QueryPlan::ScanNodes { .. }
            | QueryPlan::TraverseEdges { .. }
            | QueryPlan::Filter { .. }
            | QueryPlan::With(_)
            | QueryPlan::Return(_) => true,
            QueryPlan::CreateNode { .. }
            | QueryPlan::CreateRelationship { .. }
            | QueryPlan::MergeNode { .. }
            | QueryPlan::MergeRelationship { .. }
            | QueryPlan::Delete { .. }
            | QueryPlan::Remove(_)
            | QueryPlan::SetProperty { .. } => false,
            QueryPlan::Sequence(steps) => steps.iter().all(|s| s.is_read_only()),
        }
    }
}

pub fn plan(stmt: Statement) -> Result<QueryPlan, DbError> {
    let mut steps = Vec::new();
    let mut scope = HashSet::new();

    let mut clauses = stmt.clauses.into_iter().peekable();
    while let Some(clause) = clauses.next() {
        match clause {
            Clause::Create(pattern) => plan_create(pattern, &mut steps, &mut scope)?,
            Clause::Merge(pattern) => {
                // Collect trailing ON CREATE SET / ON MATCH SET actions.
                let mut on_create = Vec::new();
                let mut on_match = Vec::new();
                while let Some(peeked) = clauses.peek() {
                    match peeked {
                        Clause::OnCreate(_) | Clause::OnMatch(_) => match clauses.next() {
                            Some(Clause::OnCreate(s)) => {
                                validate_expression_scope(&s.value, &scope)?;
                                // ON CREATE may reference the merge variable; allow it.
                                if let Some(_v) = None::<String> {}
                                on_create.push(s);
                            }
                            Some(Clause::OnMatch(s)) => {
                                validate_expression_scope(&s.value, &scope)?;
                                on_match.push(s);
                            }
                            _ => unreachable!(),
                        },
                        _ => break,
                    }
                }
                plan_merge(pattern, on_create, on_match, &mut steps, &mut scope)?;
            }
            Clause::OnCreate(_) | Clause::OnMatch(_) => {
                return Err(DbError::QueryError(
                    "ON CREATE SET / ON MATCH SET must follow MERGE".to_string(),
                ));
            }
            Clause::Match(match_clause) => plan_match(match_clause, false, &mut steps, &mut scope)?,
            Clause::OptionalMatch(match_clause) => {
                plan_match(match_clause, true, &mut steps, &mut scope)?
            }
            Clause::Where(condition) => {
                validate_expression_scope(&condition, &scope)?;
                steps.push(QueryPlan::Filter { condition });
            }
            Clause::Set(set_clause) => plan_set(set_clause, &mut steps, &scope)?,
            Clause::Delete(delete_clause) => {
                for variable in &delete_clause.variables {
                    if !scope.contains(variable) {
                        return Err(DbError::QueryError(format!(
                            "DELETE references unknown variable `{}`",
                            variable
                        )));
                    }
                }
                steps.push(QueryPlan::Delete {
                    variables: delete_clause.variables,
                    detach: delete_clause.detach,
                });
            }
            Clause::Remove(remove_clause) => {
                for item in &remove_clause.items {
                    if !scope.contains(&item.variable) {
                        return Err(DbError::QueryError(format!(
                            "REMOVE references unknown variable `{}`",
                            item.variable
                        )));
                    }
                }
                steps.push(QueryPlan::Remove(remove_clause));
            }
            Clause::With(with_clause) => {
                for item in &with_clause.items {
                    validate_expression_scope(&item.expression, &scope)?;
                }
                for item in &with_clause.order_by {
                    validate_expression_scope(&item.expression, &scope)?;
                }
                // WITH replaces the visible scope with its projected aliases.
                let mut next_scope = HashSet::new();
                for item in &with_clause.items {
                    let name = item
                        .alias
                        .clone()
                        .unwrap_or_else(|| with_item_name(&item.expression));
                    next_scope.insert(name);
                }
                steps.push(QueryPlan::With(with_clause));
                scope = next_scope;
            }
            Clause::Return(return_clause) => {
                for item in &return_clause.items {
                    validate_expression_scope(&item.expression, &scope)?;
                }
                for item in &return_clause.order_by {
                    validate_expression_scope(&item.expression, &scope)?;
                }
                steps.push(QueryPlan::Return(return_clause));
            }
        }
    }

    Ok(QueryPlan::Sequence(steps))
}

fn with_item_name(expr: &Expression) -> String {
    match expr {
        Expression::Variable(v) => v.clone(),
        Expression::Property { variable, property } => format!("{}.{}", variable, property),
        Expression::Count(_) => "count".to_string(),
        Expression::Sum(_) => "sum".to_string(),
        Expression::Avg(_) => "avg".to_string(),
        Expression::Min(_) => "min".to_string(),
        Expression::Max(_) => "max".to_string(),
        _ => "expr".to_string(),
    }
}

fn plan_create(
    input: Pattern,
    steps: &mut Vec<QueryPlan>,
    scope: &mut HashSet<String>,
) -> Result<(), DbError> {
    match input {
        Pattern::Node(node) => {
            if let Some(variable) = &node.variable {
                scope.insert(variable.clone());
            }
            steps.push(QueryPlan::CreateNode {
                variable: node.variable.clone(),
                node,
            });
        }
        Pattern::Path(PathPattern { start, segments }) => {
            if segments.len() != 1 {
                return Err(DbError::QueryError(
                    "CREATE currently supports exactly one relationship segment".to_string(),
                ));
            }
            let first = &segments[0];
            let rel_type = first
                .relationship
                .rel_type
                .clone()
                .ok_or(DbError::QueryError(
                    "CREATE relationship requires a type".to_string(),
                ))?;
            if let Some(variable) = &start.variable {
                scope.insert(variable.clone());
            }
            if let Some(variable) = &first.relationship.variable {
                scope.insert(variable.clone());
            }
            if let Some(variable) = &first.node.variable {
                scope.insert(variable.clone());
            }
            steps.push(QueryPlan::CreateRelationship {
                src: start,
                dst: first.node.clone(),
                rel_type,
                properties: first
                    .relationship
                    .properties
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            });
        }
    }
    Ok(())
}

fn plan_merge(
    input: Pattern,
    on_create: Vec<SetClause>,
    on_match: Vec<SetClause>,
    steps: &mut Vec<QueryPlan>,
    scope: &mut HashSet<String>,
) -> Result<(), DbError> {
    // Validate ON CREATE / ON MATCH actions reference known variables
    // (the merge variable itself is added to scope first below).
    match input {
        Pattern::Node(node) => {
            if let Some(variable) = &node.variable {
                scope.insert(variable.clone());
            }
            for action in on_create.iter().chain(on_match.iter()) {
                if !scope.contains(&action.variable) {
                    return Err(DbError::QueryError(format!(
                        "ON CREATE/MATCH SET references unknown variable `{}`",
                        action.variable
                    )));
                }
                validate_expression_scope(&action.value, scope)?;
            }
            steps.push(QueryPlan::MergeNode {
                variable: node.variable.clone(),
                node,
                on_create,
                on_match,
            });
            Ok(())
        }
        Pattern::Path(path) => {
            if path.segments.len() != 1 {
                return Err(DbError::QueryError(
                    "MERGE currently supports at most one relationship segment".to_string(),
                ));
            }
            let seg = &path.segments[0];
            let rel_type = seg
                .relationship
                .rel_type
                .clone()
                .ok_or(DbError::QueryError(
                    "MERGE relationship requires a type".to_string(),
                ))?;
            if seg.relationship.hops.is_some() {
                return Err(DbError::QueryError(
                    "MERGE does not support variable-length relationships".to_string(),
                ));
            }
            if let Some(v) = &path.start.variable {
                scope.insert(v.clone());
            }
            if let Some(v) = &seg.relationship.variable {
                scope.insert(v.clone());
            }
            if let Some(v) = &seg.node.variable {
                scope.insert(v.clone());
            }
            for action in on_create.iter().chain(on_match.iter()) {
                if !scope.contains(&action.variable) {
                    return Err(DbError::QueryError(format!(
                        "ON CREATE/MATCH SET references unknown variable `{}`",
                        action.variable
                    )));
                }
                validate_expression_scope(&action.value, scope)?;
            }
            steps.push(QueryPlan::MergeRelationship {
                src: path.start.clone(),
                dst: seg.node.clone(),
                rel_type,
                rel_var: seg.relationship.variable.clone(),
                properties: seg
                    .relationship
                    .properties
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
                on_create,
                on_match,
            });
            Ok(())
        }
    }
}

fn plan_match(
    clause: MatchClause,
    optional: bool,
    steps: &mut Vec<QueryPlan>,
    scope: &mut HashSet<String>,
) -> Result<(), DbError> {
    match clause.pattern {
        Pattern::Node(node) => {
            let variable = node.variable.clone().ok_or(DbError::QueryError(
                "MATCH node requires a variable".to_string(),
            ))?;
            let filter = node_property_filter(&variable, &node.properties);
            steps.push(QueryPlan::ScanNodes {
                variable: variable.clone(),
                label: node.label.clone(),
                extra_labels: node.extra_labels.clone(),
                index_hint: node_index_hint(&variable, &node.label, &filter),
                filter,
                optional,
            });
            scope.insert(variable);
        }
        Pattern::Path(PathPattern { start, segments }) => {
            let start_variable = start.variable.clone().ok_or(DbError::QueryError(
                "MATCH path start node requires a variable".to_string(),
            ))?;
            let start_filter = node_property_filter(&start_variable, &start.properties);
            // First scan is optional only when the whole OPTIONAL MATCH has a
            // single node pattern; for paths, the start is required and each
            // traversal step preserves rows.
            let first_optional = optional && segments.is_empty();
            steps.push(QueryPlan::ScanNodes {
                variable: start_variable.clone(),
                label: start.label.clone(),
                extra_labels: start.extra_labels.clone(),
                index_hint: node_index_hint(&start_variable, &start.label, &start_filter),
                filter: start_filter,
                optional: first_optional,
            });
            scope.insert(start_variable.clone());

            let mut from_var = start_variable;
            for seg in segments {
                let to_var = seg.node.variable.clone().ok_or(DbError::QueryError(
                    "MATCH path destination node requires a variable".to_string(),
                ))?;
                let edge_var = seg.relationship.variable.clone();
                steps.push(QueryPlan::TraverseEdges {
                    from_var: from_var.clone(),
                    edge_type: seg.relationship.rel_type.clone(),
                    direction: seg.relationship.direction.clone(),
                    to_var: to_var.clone(),
                    to_label: seg.node.label.clone(),
                    to_extra_labels: seg.node.extra_labels.clone(),
                    hops: seg.relationship.hops.clone(),
                    edge_var: edge_var.clone(),
                    edge_filter: edge_property_filter(
                        edge_var.as_deref(),
                        &seg.relationship.properties,
                    ),
                    optional,
                });
                scope.insert(to_var.clone());
                if let Some(edge_var) = edge_var {
                    scope.insert(edge_var);
                }
                if let Some(filter) = node_property_filter(&to_var, &seg.node.properties) {
                    steps.push(QueryPlan::Filter { condition: filter });
                }
                // Extra labels on path destination nodes: enforce via filter on
                // label membership is handled in the executor; planner only
                // needs scope tracking here.
                from_var = to_var;
            }
        }
    }
    Ok(())
}

fn plan_set(
    clause: SetClause,
    steps: &mut Vec<QueryPlan>,
    scope: &HashSet<String>,
) -> Result<(), DbError> {
    if !scope.contains(&clause.variable) {
        return Err(DbError::QueryError(format!(
            "SET references unknown variable `{}`",
            clause.variable
        )));
    }
    validate_expression_scope(&clause.value, scope)?;
    steps.push(QueryPlan::SetProperty {
        variable: clause.variable,
        key: clause.property,
        value: clause.value,
    });
    Ok(())
}

fn node_property_filter(
    variable: &str,
    properties: &std::collections::HashMap<String, Expression>,
) -> Option<Expression> {
    let conditions = properties
        .iter()
        .map(|(key, value_expr)| Expression::BinaryOp {
            left: Box::new(Expression::Property {
                variable: variable.to_string(),
                property: key.clone(),
            }),
            op: BinaryOp::Eq,
            right: Box::new(value_expr.clone()),
        });
    and_chain(conditions.collect())
}

fn edge_property_filter(
    variable: Option<&str>,
    properties: &std::collections::HashMap<String, Expression>,
) -> Option<Expression> {
    let variable = variable?;
    node_property_filter(variable, properties)
}

fn node_index_hint(
    variable: &str,
    label: &Option<String>,
    filter: &Option<Expression>,
) -> NodeIndexHint {
    let property_match = filter
        .as_ref()
        .and_then(|expr| exact_property_match(variable, expr));
    match (label.clone(), property_match) {
        (Some(label), Some((key, value))) => NodeIndexHint::LabelAndProperty { label, key, value },
        (Some(label), None) => NodeIndexHint::Label { label },
        (None, Some((key, value))) => NodeIndexHint::Property { key, value },
        (None, None) => NodeIndexHint::FullScan,
    }
}

fn exact_property_match(variable: &str, expr: &Expression) -> Option<(String, Value)> {
    match expr {
        Expression::BinaryOp { left, op, right } if *op == BinaryOp::Eq => {
            match (&**left, &**right) {
                (
                    Expression::Property {
                        variable: prop_variable,
                        property,
                    },
                    literal,
                ) if prop_variable == variable => expression_to_literal(literal)
                    .ok()
                    .map(|value| (property.clone(), value)),
                (
                    literal,
                    Expression::Property {
                        variable: prop_variable,
                        property,
                    },
                ) if prop_variable == variable => expression_to_literal(literal)
                    .ok()
                    .map(|value| (property.clone(), value)),
                _ => None,
            }
        }
        Expression::BinaryOp { left, op, right } if *op == BinaryOp::And => {
            exact_property_match(variable, left).or_else(|| exact_property_match(variable, right))
        }
        _ => None,
    }
}

fn validate_expression_scope(expr: &Expression, scope: &HashSet<String>) -> Result<(), DbError> {
    match expr {
        Expression::Variable(variable) if !scope.contains(variable) => Err(DbError::QueryError(
            format!("expression references unknown variable `{}`", variable),
        )),
        Expression::Property { variable, .. } if !scope.contains(variable) => {
            Err(DbError::QueryError(format!(
                "expression references unknown variable `{}`",
                variable
            )))
        }
        Expression::Count(target) => match &**target {
            crate::query::ast::CountTarget::Star => Ok(()),
            crate::query::ast::CountTarget::Expr(inner) => validate_expression_scope(inner, scope),
        },
        Expression::Sum(inner)
        | Expression::Avg(inner)
        | Expression::Min(inner)
        | Expression::Max(inner) => validate_expression_scope(inner, scope),
        Expression::BinaryOp { left, right, .. } => {
            validate_expression_scope(left, scope)?;
            validate_expression_scope(right, scope)
        }
        Expression::UnaryOp { expr, .. } => validate_expression_scope(expr, scope),
        _ => Ok(()),
    }
}

fn and_chain(conditions: Vec<Expression>) -> Option<Expression> {
    conditions
        .into_iter()
        .reduce(|left, right| Expression::BinaryOp {
            left: Box::new(left),
            op: BinaryOp::And,
            right: Box::new(right),
        })
}
