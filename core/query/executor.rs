use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::db::hive_db::HiveDb;
use crate::errors::DbError;
use crate::query::ast::{BinaryOp, Direction, Expression, NodePattern, ReturnClause, UnaryOp};
use crate::query::planner::QueryPlan;
use crate::query::result::QueryResult;
use crate::storage::page::record::{NodeRecord, PropertyEntry};
use crate::transaction::Transaction;
use crate::types::{EdgeId, NodeId};
use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
enum EntityRef {
    Node(NodeId),
    Edge(EdgeId),
}

type Row = HashMap<String, EntityRef>;

pub fn execute(plan: &QueryPlan, db: &mut HiveDb) -> Result<QueryResult, DbError> {
    let mut tx = db.begin()?;
    match execute_in_tx(plan, &mut tx) {
        Ok(result) => {
            tx.commit()?;
            Ok(result)
        }
        Err(err) => {
            tx.rollback()?;
            Err(err)
        }
    }
}

fn execute_in_tx(plan: &QueryPlan, tx: &mut Transaction<'_>) -> Result<QueryResult, DbError> {
    let steps = match plan {
        QueryPlan::Sequence(steps) => steps.as_slice(),
        step => std::slice::from_ref(step),
    };
    let mut rows = vec![Row::new()];
    let mut result = QueryResult::new(Vec::new(), Vec::new());

    for step in steps {
        match step {
            QueryPlan::CreateNode { variable, node } => {
                rows = create_nodes(rows, variable, node, tx)?;
            }
            QueryPlan::CreateRelationship {
                src,
                dst,
                rel_type,
                properties,
            } => {
                rows = create_relationships(rows, src, dst, rel_type, properties, tx)?;
            }
            QueryPlan::MergeNode { variable, node } => {
                rows = merge_nodes(rows, variable, node, tx)?;
            }
            QueryPlan::ScanNodes {
                variable,
                label,
                filter,
                ..
            } => rows = scan_nodes(rows, variable, label, filter, tx)?,
            QueryPlan::TraverseEdges {
                from_var,
                edge_type,
                direction,
                to_var,
                to_label,
                edge_var,
                edge_filter,
                ..
            } => {
                rows = traverse_edges(
                    rows,
                    from_var,
                    edge_type,
                    direction,
                    to_var,
                    to_label,
                    edge_var,
                    edge_filter,
                    tx,
                )?;
            }
            QueryPlan::Filter { condition } => rows = filter_rows(rows, condition, tx)?,
            QueryPlan::SetProperty {
                variable,
                key,
                value,
            } => set_properties(&rows, variable, key, value, tx)?,
            QueryPlan::Delete { variables, detach } => {
                delete_entities(&rows, variables, *detach, tx)?;
                rows.clear();
            }
            QueryPlan::Return(return_clause) => result = project_return(&rows, return_clause, tx)?,
            QueryPlan::Sequence(_) => {
                return Err(DbError::QueryError(
                    "nested query sequences are not executable".to_string(),
                ));
            }
        }
    }

    Ok(result)
}

fn create_nodes(
    rows: Vec<Row>,
    variable: &Option<String>,
    node: &NodePattern,
    tx: &mut Transaction<'_>,
) -> Result<Vec<Row>, DbError> {
    let mut out = Vec::with_capacity(rows.len());
    let label_id = label_id_for(tx, node.label.as_deref())?;
    for mut row in rows {
        let node_id = tx.create_node_with_label(label_id)?;
        for (key, expr) in &node.properties {
            let value = eval_expr(expr, &row, tx)?;
            tx.set_node_property(node_id, key, &value)?;
        }
        if let Some(variable) = variable {
            row.insert(variable.clone(), EntityRef::Node(node_id));
        }
        out.push(row);
    }
    Ok(out)
}

fn create_relationships(
    rows: Vec<Row>,
    src: &NodePattern,
    dst: &NodePattern,
    rel_type: &str,
    properties: &[(String, Expression)],
    tx: &mut Transaction<'_>,
) -> Result<Vec<Row>, DbError> {
    let src_label = label_id_for(tx, src.label.as_deref())?;
    let dst_label = label_id_for(tx, dst.label.as_deref())?;
    let rel_label = label_id_for(tx, Some(rel_type))?;
    let mut out = Vec::with_capacity(rows.len());
    for mut row in rows {
        let src_id = get_or_create_node(&mut row, src, src_label, tx)?;
        let dst_id = get_or_create_node(&mut row, dst, dst_label, tx)?;
        let edge_id = tx.create_edge_with_label(src_id, dst_id, rel_label)?;
        for (key, expr) in properties {
            let value = eval_expr(expr, &row, tx)?;
            tx.set_edge_property(edge_id, key, &value)?;
        }
        out.push(row);
    }
    Ok(out)
}

fn get_or_create_node(
    row: &mut Row,
    pattern: &NodePattern,
    label_id: u32,
    tx: &mut Transaction<'_>,
) -> Result<NodeId, DbError> {
    if let Some(variable) = &pattern.variable
        && let Some(EntityRef::Node(node_id)) = row.get(variable)
    {
        return Ok(*node_id);
    }
    let node_id = tx.create_node_with_label(label_id)?;
    for (key, expr) in &pattern.properties {
        let value = eval_expr(expr, row, tx)?;
        tx.set_node_property(node_id, key, &value)?;
    }
    if let Some(variable) = &pattern.variable {
        row.insert(variable.clone(), EntityRef::Node(node_id));
    }
    Ok(node_id)
}

fn merge_nodes(
    rows: Vec<Row>,
    variable: &Option<String>,
    node: &NodePattern,
    tx: &mut Transaction<'_>,
) -> Result<Vec<Row>, DbError> {
    let label_id = label_id_for(tx, node.label.as_deref())?;
    let mut out = Vec::with_capacity(rows.len());
    for mut row in rows {
        if let Some((node_id, _)) = find_matching_node(label_id, &node.properties, &row, tx)? {
            if let Some(variable) = variable {
                row.insert(variable.clone(), EntityRef::Node(node_id));
            }
            out.push(row);
            continue;
        }
        let node_id = tx.create_node_with_label(label_id)?;
        for (key, expr) in &node.properties {
            let value = eval_expr(expr, &row, tx)?;
            tx.set_node_property(node_id, key, &value)?;
        }
        if let Some(variable) = variable {
            row.insert(variable.clone(), EntityRef::Node(node_id));
        }
        out.push(row);
    }
    Ok(out)
}

fn scan_nodes(
    rows: Vec<Row>,
    variable: &str,
    label: &Option<String>,
    filter: &Option<Expression>,
    tx: &mut Transaction<'_>,
) -> Result<Vec<Row>, DbError> {
    let wanted_label_id = match label {
        Some(label) => Some(label_id_for(tx, Some(label))?),
        None => None,
    };
    let nodes = tx.scan_nodes()?;
    let mut out = Vec::new();
    for row in rows {
        for (node_id, node) in &nodes {
            if wanted_label_id.is_some_and(|label_id| node.label_id != label_id) {
                continue;
            }
            if !binding_matches(&row, variable, &EntityRef::Node(*node_id))? {
                continue;
            }
            let mut next = row.clone();
            next.insert(variable.to_string(), EntityRef::Node(*node_id));
            if filter
                .as_ref()
                .is_none_or(|expr| eval_truthy(expr, &next, tx).unwrap_or(false))
            {
                out.push(next);
            }
        }
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn traverse_edges(
    rows: Vec<Row>,
    from_var: &str,
    edge_type: &Option<String>,
    direction: &Direction,
    to_var: &str,
    to_label: &Option<String>,
    edge_var: &Option<String>,
    edge_filter: &Option<Expression>,
    tx: &mut Transaction<'_>,
) -> Result<Vec<Row>, DbError> {
    let edge_label = match edge_type {
        Some(label) => Some(label_id_for(tx, Some(label))?),
        None => None,
    };
    let node_label = match to_label {
        Some(label) => Some(label_id_for(tx, Some(label))?),
        None => None,
    };
    let edges = tx.scan_edges()?;
    let mut out = Vec::new();
    for row in rows {
        let EntityRef::Node(from_id) = row.get(from_var).ok_or(DbError::QueryError(format!(
            "unbound traversal variable `{}`",
            from_var
        )))?
        else {
            return Err(DbError::QueryError(format!(
                "traversal variable `{}` is not a node",
                from_var
            )));
        };

        for (edge_id, edge) in &edges {
            if edge_label.is_some_and(|label_id| edge.label_id != label_id) {
                continue;
            }
            let candidate = match direction {
                Direction::Outgoing if edge.src == *from_id => Some(edge.dst),
                Direction::Incoming if edge.dst == *from_id => Some(edge.src),
                Direction::Undirected if edge.src == *from_id => Some(edge.dst),
                Direction::Undirected if edge.dst == *from_id => Some(edge.src),
                _ => None,
            };
            let Some(to_id) = candidate else { continue };
            let to_node = tx.get_node(to_id)?;
            if node_label.is_some_and(|label_id| to_node.label_id != label_id) {
                continue;
            }
            if !binding_matches(&row, to_var, &EntityRef::Node(to_id))? {
                continue;
            }
            if let Some(edge_var) = edge_var
                && !binding_matches(&row, edge_var, &EntityRef::Edge(*edge_id))?
            {
                continue;
            }
            let mut next = row.clone();
            next.insert(to_var.to_string(), EntityRef::Node(to_id));
            if let Some(edge_var) = edge_var {
                next.insert(edge_var.clone(), EntityRef::Edge(*edge_id));
            }
            if edge_filter
                .as_ref()
                .is_none_or(|expr| eval_truthy(expr, &next, tx).unwrap_or(false))
            {
                out.push(next);
            }
        }
    }
    Ok(out)
}

fn filter_rows(
    rows: Vec<Row>,
    condition: &Expression,
    tx: &mut Transaction<'_>,
) -> Result<Vec<Row>, DbError> {
    rows.into_iter()
        .filter_map(|row| match eval_truthy(condition, &row, tx) {
            Ok(true) => Some(Ok(row)),
            Ok(false) => None,
            Err(err) => Some(Err(err)),
        })
        .collect()
}

fn set_properties(
    rows: &[Row],
    variable: &str,
    key: &str,
    value_expr: &Expression,
    tx: &mut Transaction<'_>,
) -> Result<(), DbError> {
    for row in rows {
        let value = eval_expr(value_expr, row, tx)?;
        match row.get(variable) {
            Some(EntityRef::Node(node_id)) => tx.set_node_property(*node_id, key, &value)?,
            Some(EntityRef::Edge(edge_id)) => tx.set_edge_property(*edge_id, key, &value)?,
            None => {
                return Err(DbError::QueryError(format!(
                    "unknown variable `{}`",
                    variable
                )));
            }
        }
    }
    Ok(())
}

fn delete_entities(
    rows: &[Row],
    variables: &[String],
    detach: bool,
    tx: &mut Transaction<'_>,
) -> Result<(), DbError> {
    let mut edge_ids = HashSet::new();
    let mut node_ids = HashSet::new();
    for row in rows {
        for variable in variables {
            match row.get(variable) {
                Some(EntityRef::Node(node_id)) => {
                    node_ids.insert(*node_id);
                }
                Some(EntityRef::Edge(edge_id)) => {
                    edge_ids.insert(*edge_id);
                }
                None => {
                    return Err(DbError::QueryError(format!(
                        "unknown variable `{}`",
                        variable
                    )));
                }
            }
        }
    }

    if detach {
        for (edge_id, edge) in tx.scan_edges()? {
            if node_ids.contains(&edge.src) || node_ids.contains(&edge.dst) {
                edge_ids.insert(edge_id);
            }
        }
    }

    for edge_id in edge_ids {
        tx.delete_edge(edge_id)?;
    }
    for node_id in node_ids {
        tx.delete_node(node_id)?;
    }
    Ok(())
}

fn project_return(
    rows: &[Row],
    clause: &ReturnClause,
    tx: &mut Transaction<'_>,
) -> Result<QueryResult, DbError> {
    let columns: Vec<String> = clause
        .items
        .iter()
        .map(|item| {
            item.alias
                .clone()
                .unwrap_or_else(|| expression_name(&item.expression))
        })
        .collect();
    let mut projected = Vec::with_capacity(rows.len());
    for row in rows {
        let values = clause
            .items
            .iter()
            .map(|item| eval_expr(&item.expression, row, tx))
            .collect::<Result<Vec<_>, _>>()?;
        let order_values = clause
            .order_by
            .iter()
            .map(|item| eval_expr(&item.expression, row, tx))
            .collect::<Result<Vec<_>, _>>()?;
        projected.push((values, order_values));
    }

    if !clause.order_by.is_empty() {
        projected.sort_by(|(_, left), (_, right)| {
            for (idx, order_item) in clause.order_by.iter().enumerate() {
                let ordering = compare_values(&left[idx], &right[idx]);
                if ordering != Ordering::Equal {
                    return if order_item.descending {
                        ordering.reverse()
                    } else {
                        ordering
                    };
                }
            }
            Ordering::Equal
        });
    }

    let skip = clause.skip.unwrap_or(0);
    let limit = clause.limit.unwrap_or(usize::MAX);
    let rows = projected
        .into_iter()
        .skip(skip)
        .take(limit)
        .map(|(values, _)| values)
        .collect();
    Ok(QueryResult::new(columns, rows))
}

fn find_matching_node(
    label_id: u32,
    properties: &HashMap<String, Expression>,
    row: &Row,
    tx: &mut Transaction<'_>,
) -> Result<Option<(NodeId, NodeRecord)>, DbError> {
    for (node_id, node) in tx.scan_nodes()? {
        if node.label_id != label_id {
            continue;
        }
        let mut candidate = row.clone();
        candidate.insert("__merge".to_string(), EntityRef::Node(node_id));
        let mut matches = true;
        for (key, expr) in properties {
            let wanted = eval_expr(expr, row, tx)?;
            let got = node_property_value(tx, node_id, key)?;
            if got != wanted {
                matches = false;
                break;
            }
        }
        if matches {
            return Ok(Some((node_id, node)));
        }
    }
    Ok(None)
}

fn eval_truthy(expr: &Expression, row: &Row, tx: &mut Transaction<'_>) -> Result<bool, DbError> {
    Ok(matches!(eval_expr(expr, row, tx)?, Value::Boolean(true)))
}

fn eval_expr(expr: &Expression, row: &Row, tx: &mut Transaction<'_>) -> Result<Value, DbError> {
    match expr {
        Expression::Integer(n) => Ok(Value::Integer(*n)),
        Expression::Float(n) => Ok(Value::Float(*n)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Boolean(b) => Ok(Value::Boolean(*b)),
        Expression::Variable(variable) => match row.get(variable) {
            Some(EntityRef::Node(node_id)) => Ok(Value::Integer(*node_id as i64)),
            Some(EntityRef::Edge(edge_id)) => Ok(Value::Integer(*edge_id as i64)),
            None => Ok(Value::Null),
        },
        Expression::Property { variable, property } => match row.get(variable) {
            Some(EntityRef::Node(node_id)) => node_property_value(tx, *node_id, property),
            Some(EntityRef::Edge(edge_id)) => edge_property_value(tx, *edge_id, property),
            _ => Ok(Value::Null),
        },
        Expression::BinaryOp { left, op, right } => {
            let left = eval_expr(left, row, tx)?;
            let right = eval_expr(right, row, tx)?;
            eval_binary(left, op, right)
        }
        Expression::UnaryOp { op, expr } => match op {
            UnaryOp::Not => Ok(Value::Boolean(!matches!(
                eval_expr(expr, row, tx)?,
                Value::Boolean(true)
            ))),
        },
    }
}

fn eval_binary(left: Value, op: &BinaryOp, right: Value) -> Result<Value, DbError> {
    match op {
        BinaryOp::Eq => Ok(Value::Boolean(left == right)),
        BinaryOp::Neq => Ok(Value::Boolean(left != right)),
        BinaryOp::Gt => Ok(Value::Boolean(
            compare_values(&left, &right) == Ordering::Greater,
        )),
        BinaryOp::Gte => Ok(Value::Boolean(matches!(
            compare_values(&left, &right),
            Ordering::Greater | Ordering::Equal
        ))),
        BinaryOp::Lt => Ok(Value::Boolean(
            compare_values(&left, &right) == Ordering::Less,
        )),
        BinaryOp::Lte => Ok(Value::Boolean(matches!(
            compare_values(&left, &right),
            Ordering::Less | Ordering::Equal
        ))),
        BinaryOp::And => Ok(Value::Boolean(
            matches!(left, Value::Boolean(true)) && matches!(right, Value::Boolean(true)),
        )),
        BinaryOp::Or => Ok(Value::Boolean(
            matches!(left, Value::Boolean(true)) || matches!(right, Value::Boolean(true)),
        )),
    }
}

fn compare_values(left: &Value, right: &Value) -> Ordering {
    match (left, right) {
        (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
        (Value::Integer(a), Value::Float(b)) => {
            (*a as f64).partial_cmp(b).unwrap_or(Ordering::Equal)
        }
        (Value::Float(a), Value::Integer(b)) => {
            a.partial_cmp(&(*b as f64)).unwrap_or(Ordering::Equal)
        }
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn node_property_value(
    tx: &mut Transaction<'_>,
    node_id: NodeId,
    key: &str,
) -> Result<Value, DbError> {
    match tx.get_node_property(node_id, key) {
        Ok(value) => Ok(value),
        Err(DbError::ReadError) => Ok(Value::Null),
        Err(err) => Err(err),
    }
}

fn edge_property_value(
    tx: &mut Transaction<'_>,
    edge_id: EdgeId,
    key: &str,
) -> Result<Value, DbError> {
    match tx.get_edge_property(edge_id, key) {
        Ok(value) => Ok(value),
        Err(DbError::ReadError) => Ok(Value::Null),
        Err(err) => Err(err),
    }
}

fn binding_matches(row: &Row, variable: &str, candidate: &EntityRef) -> Result<bool, DbError> {
    Ok(row
        .get(variable)
        .is_none_or(|existing| existing == candidate))
}

fn label_id_for(tx: &mut Transaction<'_>, label: Option<&str>) -> Result<u32, DbError> {
    match label {
        Some(label) => tx.register_label(label),
        None => Ok(0),
    }
}

fn expression_name(expr: &Expression) -> String {
    match expr {
        Expression::Variable(variable) => variable.clone(),
        Expression::Property { variable, property } => format!("{}.{}", variable, property),
        _ => "expr".to_string(),
    }
}

#[allow(dead_code)]
fn inline_property_value(entry: &PropertyEntry) -> Value {
    Value::from_bytes(entry.value_type, entry.value_inline)
}
