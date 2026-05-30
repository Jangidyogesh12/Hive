use std::collections::{HashMap, HashSet, VecDeque};

use crate::db::hive_db::{HiveDb, Node, Property};
use crate::errors::DbError;
use crate::query::ast::{
    BinaryOp, Direction, Expression, NodePattern, RelationshipLength, ReturnItem, UnaryOp,
};
use crate::query::planner::QueryPlan;
use crate::query::types::QueryResult;
use crate::query::utils::expression_to_literal;
use crate::types::{DELETED, EdgeId, NIL_ID, NodeId};
use crate::value::{self, Value};

type Row = HashMap<String, u64>;

pub struct Executor<'a> {
    db: &'a mut HiveDb,
}

impl<'a> Executor<'a> {
    /// Creates a new executor backed by the given `HiveDb` reference.
    pub fn new(db: &'a mut HiveDb) -> Self {
        Executor { db }
    }

    /// Executes a top-level query plan and returns the result rows.
    pub fn execute(&mut self, plan: QueryPlan) -> Result<QueryResult, DbError> {
        match plan {
            QueryPlan::CreateNode { node } => {
                self.exec_create_node(node)?;
                Ok(QueryResult::new(vec![], vec![]))
            }
            QueryPlan::MergeNode { node } => {
                self.exec_merge_node(node)?;
                Ok(QueryResult::new(vec![], vec![]))
            }
            QueryPlan::CreateRelationship {
                src,
                dst,
                rel_type,
                properties,
            } => {
                self.exec_create_edge(src, dst, rel_type, properties)?;
                Ok(QueryResult::new(vec![], vec![]))
            }
            QueryPlan::Sequence(steps) => self.exec_sequence(steps),
            QueryPlan::DeleteEntity { variable } => self.exec_delete(&variable),
            QueryPlan::SetProperty {
                variable,
                key,
                value,
            } => self.exec_set_property(&variable, &key, value),
            other => {
                let rows = self.exec_plan_step(other, &[HashMap::new()])?;
                self.rows_to_result(&rows)
            }
        }
    }

    /// Creates a node via `HiveDb::create_node` from the planner's label and properties.
    fn exec_create_node(&mut self, node: NodePattern) -> Result<NodeId, DbError> {
        let mut props: Vec<Property> = Vec::new();

        for (key, expr) in &node.properties {
            let key_hash = value::hash_key(key);
            let value = expression_to_literal(expr)?;
            let (value_type, value_inline) = value.to_inline_bytes();
            props.push(Property {
                key_value: key.clone(),
                key_hash,
                value_type,
                value_inline,
            })
        }

        let node_id = self.db.create_node(&node.label.unwrap(), props)?;
        Ok(node_id)
    }

    fn exec_merge_node(&mut self, node: NodePattern) -> Result<NodeId, DbError> {
        let label = node.label.clone().ok_or(DbError::QueryError(
            "MERGE requires a node label".to_string(),
        ))?;

        let mut expected: Vec<(String, Value)> = Vec::new();

        for (k, expr) in &node.properties {
            expected.push((k.clone(), expression_to_literal(expr)?));
        }

        let count = self.db.node_count()?;

        for id in 0..count {
            let rec = self.db.get_node(id)?;

            if (rec.flags & DELETED) != 0 {
                continue;
            }
            if rec.label != label {
                continue;
            }

            let mut all_match = true;

            for (k, v_expected) in &expected {
                let v_actual = self.db.get_node_property(id, k)?;
                match v_actual {
                    Some(v) if v == *v_expected => {}
                    _ => {
                        all_match = false;
                        break;
                    }
                }
            }
            if all_match {
                return Ok(id);
            }
        }

        self.exec_create_node(node)
    }

    /// Creates a Relation Ship with node Creation via `HiveDb::create_edge`.
    fn exec_create_edge(
        &mut self,
        src: NodePattern,
        dst: NodePattern,
        label: String,
        properties: Vec<(String, Value)>,
    ) -> Result<EdgeId, DbError> {
        let src_node_id = self.exec_create_node(src)?;

        let dst_node_id = self.exec_create_node(dst)?;

        let props: Vec<Property> = properties
            .iter()
            .map(|(k, v)| {
                let key_hash = value::hash_key(k);
                let (value_type, value_inline) = v.to_inline_bytes();
                Property {
                    key_value: k.clone(),
                    key_hash,
                    value_type,
                    value_inline,
                }
            })
            .collect();

        self.db
            .create_edge(src_node_id, dst_node_id, label.as_str(), props)
    }

    /// Executes a sequence of plan steps, piping intermediate row sets
    /// through each step until a Return step is reached.
    fn exec_sequence(&mut self, steps: Vec<QueryPlan>) -> Result<QueryResult, DbError> {
        let mut rows: Vec<Row> = vec![HashMap::new()];

        for step in steps {
            match step {
                QueryPlan::Return { items } => {
                    return self.exec_return(&items, &rows);
                }
                _ => {
                    rows = self.exec_plan_step(step, &rows)?;
                }
            }
        }

        self.rows_to_result(&rows)
    }

    /// Converts raw `Row` maps into a `QueryResult` with columns derived from keys.
    fn rows_to_result(&self, rows: &[Row]) -> Result<QueryResult, DbError> {
        let columns: Vec<String> = rows
            .first()
            .map(|r| r.keys().cloned().collect())
            .unwrap_or_default();
        let result_rows: Vec<Vec<Value>> = rows
            .iter()
            .map(|r| {
                columns
                    .iter()
                    .map(|col| {
                        r.get(col)
                            .map(|&id| Value::Integer(id as i64))
                            .unwrap_or(Value::Null)
                    })
                    .collect()
            })
            .collect();
        Ok(QueryResult::new(columns, result_rows))
    }

    /// Dispatches a single plan step against the current row set,
    /// returning the new row set produced by that step.
    fn exec_plan_step(&mut self, step: QueryPlan, rows: &[Row]) -> Result<Vec<Row>, DbError> {
        match step {
            QueryPlan::ScanNodes {
                variable,
                label,
                filter,
            } => {
                let mut new_rows = Vec::new();
                for row in rows {
                    let scanned = self.scan_nodes(&variable, &label, &filter)?;
                    for s_row in scanned {
                        let mut combined = row.clone();
                        combined.extend(s_row);
                        new_rows.push(combined);
                    }
                }
                Ok(new_rows)
            }
            QueryPlan::TraverseEdges {
                from_var,
                edge_type,
                direction,
                to_var,
                to_label,
                hops,
            } => {
                let mut new_rows = Vec::new();
                for row in rows {
                    let traversed = self.traverse_edges(
                        row, &from_var, &edge_type, &direction, &to_var, &to_label, &hops,
                    )?;
                    new_rows.extend(traversed);
                }
                Ok(new_rows)
            }
            QueryPlan::Filter { condition } => {
                let mut new_rows = Vec::new();
                for row in rows {
                    if self.eval_condition(&condition, row)? {
                        new_rows.push(row.clone());
                    }
                }
                Ok(new_rows)
            }
            QueryPlan::DeleteEntity { variable } => {
                for row in rows {
                    if let Some(&id) = row.get(&variable) {
                        self.db.delete_node(id)?;
                    }
                }
                Ok(rows.to_vec())
            }
            QueryPlan::SetProperty {
                variable,
                key,
                value,
            } => {
                for row in rows {
                    if let Some(&id) = row.get(&variable) {
                        self.db.set_node_property(id, &key, value.clone())?;
                    }
                }
                Ok(rows.to_vec())
            }
            QueryPlan::Sequence(steps) => {
                let mut current = rows.to_vec();
                for step in steps {
                    match step {
                        QueryPlan::Return { .. } => {
                            return Ok(current);
                        }
                        _ => {
                            current = self.exec_plan_step(step, &current)?;
                        }
                    }
                }
                Ok(current)
            }
            _ => Ok(rows.to_vec()),
        }
    }

    /// Scans all nodes, skipping deleted ones, and collects those matching
    /// the given label and optional filter expression.
    fn scan_nodes(
        &mut self,
        variable: &str,
        label: &Option<String>,
        filter: &Option<Expression>,
    ) -> Result<Vec<Row>, DbError> {
        let count = self.db.node_count()?;
        let mut rows = Vec::new();

        for id in 0..count {
            let node = self.db.get_node(id)?;
            if (node.flags & DELETED) != 0 {
                continue;
            }

            let label_match = label
                .as_ref()
                .map_or(true, |l| node.label.as_str() == l.as_str());

            if !label_match {
                continue;
            }

            let row = {
                let mut m = HashMap::new();
                m.insert(variable.to_string(), id);
                m
            };

            if let Some(pred) = filter {
                if !self.eval_condition(pred, &row)? {
                    continue;
                }
            }

            rows.push(row);
        }

        Ok(rows)
    }

    /// Traverses edges from a bound row variable in the specified direction,
    /// filtering by edge type and target node label.
    fn traverse_edges(
        &mut self,
        row: &Row,
        from_var: &str,
        edge_type: &Option<String>,
        direction: &Direction,
        to_var: &str,
        to_label: &Option<String>,
        hops: &Option<RelationshipLength>,
    ) -> Result<Vec<Row>, DbError> {
        let from_id = match row.get(from_var) {
            Some(&id) => id,
            None => return Ok(vec![]),
        };

        let (min_hops, max_hops) = match hops {
            Some(h) => (h.min_hops.unwrap_or(1), h.max_hops.unwrap_or(1)),
            None => (1, 1),
        };

        if min_hops > max_hops {
            return Ok(vec![]);
        }

        let mut results = Vec::new();

        let mut queue: VecDeque<(u64, u32)> = VecDeque::new();
        let mut visited: HashSet<u64> = HashSet::new();

        queue.push_back((from_id, 0));
        visited.insert(from_id);

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth >= max_hops {
                continue;
            }

            let next_depth = depth + 1;
            let neighbors = self.collect_neighbors(current_id, edge_type, direction, to_label)?;

            for neighbor_id in neighbors {
                if visited.insert(neighbor_id) {
                    if next_depth >= min_hops && next_depth <= max_hops {
                        let mut new_row = row.clone();
                        new_row.insert(to_var.to_string(), neighbor_id);
                        results.push(new_row);
                    }
                    queue.push_back((neighbor_id, next_depth));
                }
            }
        }

        Ok(results)
    }

    fn collect_neighbors(
        &mut self,
        from_id: u64,
        edge_type: &Option<String>,
        direction: &Direction,
        to_label: &Option<String>,
    ) -> Result<Vec<u64>, DbError> {
        let node = self.db.get_node(from_id)?;
        let mut neighbors = Vec::new();

        match direction {
            Direction::Outgoing => {
                self.walk_edges(
                    node.first_out_edge,
                    from_id,
                    false,
                    edge_type,
                    to_label,
                    &mut neighbors,
                )?;
            }
            Direction::Incoming => {
                self.walk_edges(
                    node.first_in_edge,
                    from_id,
                    true,
                    edge_type,
                    to_label,
                    &mut neighbors,
                )?;
            }
            Direction::Undirected => {
                self.walk_edges(
                    node.first_out_edge,
                    from_id,
                    false,
                    edge_type,
                    to_label,
                    &mut neighbors,
                )?;
                self.walk_edges(
                    node.first_in_edge,
                    from_id,
                    true,
                    edge_type,
                    to_label,
                    &mut neighbors,
                )?;
            }
        }

        Ok(neighbors)
    }

    /// Walks a linked list of edges from `first_edge`, collecting neighbours
    /// that match the edge type and target label filters, skipping deleted edges.
    fn walk_edges(
        &mut self,
        first_edge: u64,
        from_id: u64,
        incoming: bool,
        edge_type: &Option<String>,
        to_label: &Option<String>,
        neighbors: &mut Vec<u64>,
    ) -> Result<(), DbError> {
        let mut edge_id = first_edge;

        while edge_id != NIL_ID {
            let edge = self.db.get_edge(edge_id)?;

            let next_id = if incoming {
                edge.next_in_edge
            } else {
                edge.next_out_edge
            };

            if (edge.flags & DELETED) != 0 {
                edge_id = next_id;
                continue;
            }

            let type_match = edge_type
                .as_ref()
                .map_or(true, |t| edge.label.as_str() == t.as_str());

            if !type_match {
                edge_id = next_id;
                continue;
            }

            let neighbor_id = if incoming { edge.src } else { edge.dst };

            if neighbor_id == from_id {
                edge_id = next_id;
                continue;
            }

            if let Some(lbl) = to_label {
                let dst_node = self.db.get_node(neighbor_id)?;
                if dst_node.label.as_str() != lbl.as_str() {
                    edge_id = next_id;
                    continue;
                }
            }

            neighbors.push(neighbor_id);

            edge_id = next_id;
        }

        Ok(())
    }

    /// Evaluates RETURN items against the bound rows, producing the final
    /// `QueryResult` with resolved column names and values.
    fn exec_return(&mut self, items: &[ReturnItem], rows: &[Row]) -> Result<QueryResult, DbError> {
        let columns: Vec<String> = items
            .iter()
            .map(|item| {
                item.alias
                    .clone()
                    .unwrap_or_else(|| return_item_column_name(&item.expression))
            })
            .collect();

        let mut result_rows: Vec<Vec<Value>> = Vec::new();

        for row in rows {
            let mut result_row: Vec<Value> = Vec::new();
            for item in items {
                let val = self.eval_expression(&item.expression, row)?;
                result_row.push(val);
            }
            result_rows.push(result_row);
        }

        Ok(QueryResult::new(columns, result_rows))
    }

    /// Handles a standalone DELETE by resolving the variable from bound rows
    /// and calling `HiveDb::delete_node`. Returns an error if no preceding
    /// MATCH has bound the variable.
    fn exec_delete(&mut self, variable: &str) -> Result<QueryResult, DbError> {
        let _ = variable;
        Err(DbError::QueryError(format!(
            "DELETE requires a preceding MATCH to bind the variable"
        )))
    }

    /// Handles a standalone SET by resolving the variable from bound rows.
    /// Returns an error if no preceding MATCH has bound the variable.
    fn exec_set_property(
        &mut self,
        variable: &str,
        _key: &str,
        _value: Value,
    ) -> Result<QueryResult, DbError> {
        let _ = variable;
        Err(DbError::QueryError(format!(
            "SET requires a preceding MATCH to bind the variable"
        )))
    }

    /// Evaluates an expression as a boolean condition against a row.
    fn eval_condition(&mut self, expr: &Expression, row: &Row) -> Result<bool, DbError> {
        let val = self.eval_expression(expr, row)?;
        match val {
            Value::Boolean(b) => Ok(b),
            _ => Ok(false),
        }
    }

    /// Evaluates an expression tree to a concrete `Value`, resolving
    /// variables from the given row and property access via `HiveDb`.
    fn eval_expression(&mut self, expr: &Expression, row: &Row) -> Result<Value, DbError> {
        match expr {
            Expression::Integer(n) => Ok(Value::Integer(*n)),
            Expression::Float(f) => Ok(Value::Float(*f)),
            Expression::String(s) => Ok(Value::String(s.clone())),
            Expression::Boolean(b) => Ok(Value::Boolean(*b)),
            Expression::Variable(name) => match row.get(name) {
                Some(&id) => Ok(Value::Integer(id as i64)),
                None => Err(DbError::QueryError(format!(
                    "Variable '{}' is not bound",
                    name
                ))),
            },
            Expression::Property { variable, property } => match row.get(variable) {
                Some(&id) => self
                    .db
                    .get_node_property(id, property)?
                    .ok_or(DbError::QueryError(format!(
                        "Property '{}' not found on '{}'",
                        property, variable
                    ))),
                None => Err(DbError::QueryError(format!(
                    "Variable '{}' is not bound",
                    variable
                ))),
            },
            Expression::BinaryOp { left, op, right } => {
                let l = self.eval_expression(left, row)?;
                let r = self.eval_expression(right, row)?;
                Ok(eval_binary_op(&l, op, &r))
            }
            Expression::UnaryOp { op, expr } => {
                let v = self.eval_expression(expr, row)?;
                Ok(eval_unary_op(op, &v))
            }
        }
    }
}

/// Evaluates a binary operation on two values, returning a `Value::Boolean`
/// for comparisons and logical operators.
fn eval_binary_op(left: &Value, op: &BinaryOp, right: &Value) -> Value {
    match op {
        BinaryOp::Eq => Value::Boolean(left == right),
        BinaryOp::Neq => Value::Boolean(left != right),
        BinaryOp::And => {
            let l = matches!(left, Value::Boolean(true));
            let r = matches!(right, Value::Boolean(true));
            Value::Boolean(l && r)
        }
        BinaryOp::Or => {
            let l = matches!(left, Value::Boolean(true));
            let r = matches!(right, Value::Boolean(true));
            Value::Boolean(l || r)
        }
        BinaryOp::Gt => compare_values(left, right, std::cmp::Ordering::Greater),
        BinaryOp::Gte => {
            let ord = cmp_values(left, right);
            Value::Boolean(
                ord == Some(std::cmp::Ordering::Greater) || ord == Some(std::cmp::Ordering::Equal),
            )
        }
        BinaryOp::Lt => compare_values(left, right, std::cmp::Ordering::Less),
        BinaryOp::Lte => {
            let ord = cmp_values(left, right);
            Value::Boolean(
                ord == Some(std::cmp::Ordering::Less) || ord == Some(std::cmp::Ordering::Equal),
            )
        }
    }
}

/// Compares two values against a target ordering, returning `Value::Boolean`.
fn compare_values(left: &Value, right: &Value, target: std::cmp::Ordering) -> Value {
    Value::Boolean(cmp_values(left, right) == Some(target))
}

/// Compares two values, returning `Some(Ordering)` when the types are
/// comparable, or `None` for incomparable type combinations.
fn cmp_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Integer(l), Value::Integer(r)) => Some(l.cmp(r)),
        (Value::Float(l), Value::Float(r)) => l.partial_cmp(r),
        (Value::Integer(l), Value::Float(r)) => (*l as f64).partial_cmp(r),
        (Value::Float(l), Value::Integer(r)) => l.partial_cmp(&(*r as f64)),
        (Value::String(l), Value::String(r)) => Some(l.cmp(r)),
        _ => None,
    }
}

/// Evaluates a unary operation (NOT) on a value.
fn eval_unary_op(op: &UnaryOp, val: &Value) -> Value {
    match op {
        UnaryOp::Not => match val {
            Value::Boolean(b) => Value::Boolean(!b),
            _ => Value::Boolean(false),
        },
    }
}

/// Derives a human-readable column name from a return expression.
fn return_item_column_name(expr: &Expression) -> String {
    match expr {
        Expression::Variable(name) => name.clone(),
        Expression::Property { variable, property } => format!("{}.{}", variable, property),
        _ => "expr".to_string(),
    }
}
