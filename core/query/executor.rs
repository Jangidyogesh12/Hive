use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::db::hive_db::HiveDb;
use crate::errors::DbError;
use crate::query::ast::{
    BinaryOp, CountTarget, Direction, Expression, NodePattern, RemoveClause, ReturnClause,
    ReturnItem, SetClause, UnaryOp,
};
use crate::query::planner::{NodeIndexHint, QueryPlan};
use crate::query::result::QueryResult;
use crate::storage::page::record::{NodeRecord, PropertyEntry};
use crate::transaction::Transaction;
use crate::types::{EdgeId, NodeId};
use crate::value::Value;

/// A binding of a query variable to an entity or a computed value.
#[derive(Debug, Clone, PartialEq)]
enum Binding {
    /// A node reference, identified by its packed `NodeId`.
    Node(NodeId),
    /// An edge reference, identified by its packed `EdgeId`.
    Edge(EdgeId),
    /// A computed value (from `WITH` projections, literals, params).
    Value(Value),
}

/// A single row of variable bindings produced during query execution.
type Row = HashMap<String, Binding>;

/// Parameters bound to `$name` placeholders.
pub type Params = HashMap<String, Value>;

/// Maximum hops explored for unbounded variable-length traversals (`*`, `*2..`).
pub const MAX_VAR_HOPS: u32 = 8;

pub fn execute(plan: &QueryPlan, db: &mut HiveDb) -> Result<QueryResult, DbError> {
    execute_with_params(plan, db, &HashMap::new())
}

pub fn execute_with_params(
    plan: &QueryPlan,
    db: &mut HiveDb,
    params: &Params,
) -> Result<QueryResult, DbError> {
    let mut tx = db.begin()?;
    let readonly = plan.is_read_only();
    match execute_in_tx(plan, &mut tx, params) {
        Ok(result) => {
            if readonly {
                tx.commit_readonly()?;
            } else {
                tx.commit()?;
            }
            Ok(result)
        }
        Err(err) => {
            tx.rollback()?;
            Err(err)
        }
    }
}

/// Returns a human-readable rendering of a query plan (`EXPLAIN`).
pub fn explain_plan(plan: &QueryPlan) -> String {
    fn fmt(plan: &QueryPlan, indent: usize, out: &mut String) {
        let pad = "  ".repeat(indent);
        match plan {
            QueryPlan::Sequence(steps) => {
                out.push_str(&format!("{}Sequence ({} steps)\n", pad, steps.len()));
                for step in steps {
                    fmt(step, indent + 1, out);
                }
            }
            other => out.push_str(&format!("{}{:?}\n", pad, other)),
        }
    }
    let mut out = String::new();
    fmt(plan, 0, &mut out);
    out
}

fn execute_in_tx(
    plan: &QueryPlan,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<QueryResult, DbError> {
    let steps = match plan {
        QueryPlan::Sequence(steps) => steps.as_slice(),
        step => std::slice::from_ref(step),
    };
    let mut rows = vec![Row::new()];
    let mut result = QueryResult::new(Vec::new(), Vec::new());

    for step in steps {
        match step {
            QueryPlan::CreateNode { variable, node } => {
                rows = create_nodes(rows, variable, node, tx, params)?;
            }
            QueryPlan::CreateRelationship {
                src,
                dst,
                rel_type,
                properties,
            } => {
                rows = create_relationships(rows, src, dst, rel_type, properties, tx, params)?;
            }
            QueryPlan::MergeNode {
                variable,
                node,
                on_create,
                on_match,
            } => {
                rows = merge_nodes(rows, variable, node, on_create, on_match, tx, params)?;
            }
            QueryPlan::MergeRelationship {
                src,
                dst,
                rel_type,
                rel_var,
                properties,
                on_create,
                on_match,
            } => {
                rows = merge_relationships(
                    rows, src, dst, rel_type, rel_var, properties, on_create, on_match, tx, params,
                )?;
            }
            QueryPlan::ScanNodes {
                variable,
                label,
                extra_labels,
                filter,
                index_hint,
                optional,
            } => {
                rows = scan_nodes(
                    rows,
                    variable,
                    label,
                    extra_labels,
                    filter,
                    index_hint,
                    *optional,
                    tx,
                    params,
                )?
            }
            QueryPlan::TraverseEdges {
                from_var,
                edge_type,
                direction,
                to_var,
                to_label,
                to_extra_labels,
                hops,
                edge_var,
                edge_filter,
                optional,
            } => {
                rows = traverse_edges(
                    rows,
                    from_var,
                    edge_type,
                    direction,
                    to_var,
                    to_label,
                    to_extra_labels,
                    hops,
                    edge_var,
                    edge_filter,
                    *optional,
                    tx,
                    params,
                )?;
            }
            QueryPlan::Filter { condition } => rows = filter_rows(rows, condition, tx, params)?,
            QueryPlan::With(with_clause) => rows = project_with(&rows, with_clause, tx, params)?,
            QueryPlan::SetProperty {
                variable,
                key,
                value,
            } => set_properties(&rows, variable, key, value, tx, params)?,
            QueryPlan::Delete { variables, detach } => {
                delete_entities(&rows, variables, *detach, tx)?;
                rows.clear();
            }
            QueryPlan::Remove(remove_clause) => {
                remove_entities(&rows, remove_clause, tx)?;
            }
            QueryPlan::Return(return_clause) => {
                result = project_return(&rows, return_clause, tx, params)?
            }
            QueryPlan::Sequence(_) => {
                return Err(DbError::QueryError(
                    "nested query sequences are not executable".to_string(),
                ));
            }
        }
    }

    Ok(result)
}

/// Creates one new node per row, optionally assigning properties and binding to a variable.
fn create_nodes(
    rows: Vec<Row>,
    variable: &Option<String>,
    node: &NodePattern,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    let mut out = Vec::with_capacity(rows.len());
    let label_id = label_id_for(tx, node.label.as_deref())?;
    let mut extra_ids = Vec::with_capacity(node.extra_labels.len());
    for extra in &node.extra_labels {
        extra_ids.push(tx.register_label(extra)?);
    }
    for mut row in rows {
        let node_id = tx.create_node_with_label(label_id)?;
        for extra_id in &extra_ids {
            let name = tx
                .get_label_name(*extra_id)?
                .unwrap_or_else(|| format!("label_{extra_id}"));
            tx.add_node_label(node_id, &name)?;
        }
        for (key, expr) in &node.properties {
            let value = eval_expr(expr, &row, tx, params)?;
            check_unique_constraint_multi(tx, node_id, key, &value)?;
            tx.set_node_property(node_id, key, &value)?;
        }
        if let Some(variable) = variable {
            row.insert(variable.clone(), Binding::Node(node_id));
        }
        out.push(row);
    }
    Ok(out)
}

/// Creates a directed edge between two nodes per row, resolving or creating the src/dst as needed.
fn create_relationships(
    rows: Vec<Row>,
    src: &NodePattern,
    dst: &NodePattern,
    rel_type: &str,
    properties: &[(String, Expression)],
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    let src_label = label_id_for(tx, src.label.as_deref())?;
    let dst_label = label_id_for(tx, dst.label.as_deref())?;
    let rel_label = label_id_for(tx, Some(rel_type))?;
    let mut out = Vec::with_capacity(rows.len());
    for mut row in rows {
        let src_id = get_or_create_node(&mut row, src, src_label, tx, params)?;
        let dst_id = get_or_create_node(&mut row, dst, dst_label, tx, params)?;
        let edge_id = tx.create_edge_with_label(src_id, dst_id, rel_label)?;
        for (key, expr) in properties {
            let value = eval_expr(expr, &row, tx, params)?;
            tx.set_edge_property(edge_id, key, &value)?;
        }
        out.push(row);
    }
    Ok(out)
}

/// Returns the existing node ID for a variable if bound, or creates a new node and binds it.
fn get_or_create_node(
    row: &mut Row,
    pattern: &NodePattern,
    label_id: u32,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<NodeId, DbError> {
    if let Some(variable) = &pattern.variable
        && let Some(Binding::Node(node_id)) = row.get(variable)
    {
        return Ok(*node_id);
    }
    let node_id = tx.create_node_with_label(label_id)?;
    for extra in &pattern.extra_labels {
        tx.add_node_label(node_id, extra)?;
    }
    for (key, expr) in &pattern.properties {
        let value = eval_expr(expr, row, tx, params)?;
        tx.set_node_property(node_id, key, &value)?;
    }
    if let Some(variable) = &pattern.variable {
        row.insert(variable.clone(), Binding::Node(node_id));
    }
    Ok(node_id)
}

/// Resolves an endpoint for relationship `MERGE` with match-or-create
/// semantics: reuses a bound variable, matches an existing node by label and
/// inline properties, or creates a new node. This is what makes repeated
/// relationship `MERGE` deterministic instead of duplicating endpoints.
fn merge_get_node(
    row: &mut Row,
    pattern: &NodePattern,
    label_id: u32,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<NodeId, DbError> {
    if let Some(variable) = &pattern.variable
        && let Some(Binding::Node(node_id)) = row.get(variable)
    {
        return Ok(*node_id);
    }
    if let Some((node_id, _)) = find_matching_node(label_id, &pattern.properties, row, tx, params)?
    {
        // Ensure extra labels match too: a candidate missing any requested
        // extra label is not a match (MERGE creates a fully-labeled node).
        let node = tx.get_node(node_id)?;
        let mut extras_ok = true;
        for extra in &pattern.extra_labels {
            let extra_id = tx.register_label(extra)?;
            if !node.has_label(extra_id) {
                extras_ok = false;
                break;
            }
        }
        if extras_ok {
            if let Some(variable) = &pattern.variable {
                row.insert(variable.clone(), Binding::Node(node_id));
            }
            return Ok(node_id);
        }
    }
    let node_id = tx.create_node_with_label(label_id)?;
    for extra in &pattern.extra_labels {
        tx.add_node_label(node_id, extra)?;
    }
    for (key, expr) in &pattern.properties {
        let value = eval_expr(expr, row, tx, params)?;
        check_unique_constraint_multi(tx, node_id, key, &value)?;
        tx.set_node_property(node_id, key, &value)?;
    }
    if let Some(variable) = &pattern.variable {
        row.insert(variable.clone(), Binding::Node(node_id));
    }
    Ok(node_id)
}

/// Merges nodes: reuses an existing node matching the label and properties, or creates a new one.
/// Applies `ON CREATE SET` when a node is created and `ON MATCH SET` when reused.
#[allow(clippy::too_many_arguments)]
fn merge_nodes(
    rows: Vec<Row>,
    variable: &Option<String>,
    node: &NodePattern,
    on_create: &[SetClause],
    on_match: &[SetClause],
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    let label_id = label_id_for(tx, node.label.as_deref())?;
    let mut extra_ids = Vec::with_capacity(node.extra_labels.len());
    for extra in &node.extra_labels {
        extra_ids.push(tx.register_label(extra)?);
    }
    let mut out = Vec::with_capacity(rows.len());
    for mut row in rows {
        if let Some((node_id, _)) =
            find_matching_node(label_id, &node.properties, &row, tx, params)?
        {
            if let Some(variable) = variable {
                row.insert(variable.clone(), Binding::Node(node_id));
            }
            apply_set_actions(&row, on_match, tx, params)?;
            // Re-read row binding after actions (actions mutate storage, not the row).
            out.push(row);
            continue;
        }
        let node_id = tx.create_node_with_label(label_id)?;
        for extra_id in &extra_ids {
            let name = tx
                .get_label_name(*extra_id)?
                .unwrap_or_else(|| format!("label_{extra_id}"));
            tx.add_node_label(node_id, &name)?;
        }
        for (key, expr) in &node.properties {
            let value = eval_expr(expr, &row, tx, params)?;
            check_unique_constraint_multi(tx, node_id, key, &value)?;
            tx.set_node_property(node_id, key, &value)?;
        }
        if let Some(variable) = variable {
            row.insert(variable.clone(), Binding::Node(node_id));
        }
        apply_set_actions(&row, on_create, tx, params)?;
        out.push(row);
    }
    Ok(out)
}

/// Deterministic relationship MERGE: matches an edge between resolved endpoints
/// with the same type and inline properties, otherwise creates it.
#[allow(clippy::too_many_arguments)]
fn merge_relationships(
    rows: Vec<Row>,
    src: &NodePattern,
    dst: &NodePattern,
    rel_type: &str,
    rel_var: &Option<String>,
    properties: &[(String, Expression)],
    on_create: &[SetClause],
    on_match: &[SetClause],
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    let src_label = label_id_for(tx, src.label.as_deref())?;
    let dst_label = label_id_for(tx, dst.label.as_deref())?;
    let rel_label = label_id_for(tx, Some(rel_type))?;
    let mut out = Vec::with_capacity(rows.len());
    for mut row in rows {
        let src_id = merge_get_node(&mut row, src, src_label, tx, params)?;
        let dst_id = merge_get_node(&mut row, dst, dst_label, tx, params)?;
        // Evaluate wanted edge properties once.
        let mut wanted: Vec<(String, Value)> = Vec::with_capacity(properties.len());
        for (key, expr) in properties {
            wanted.push((key.clone(), eval_expr(expr, &row, tx, params)?));
        }
        // Search outgoing chain for a deterministic match.
        let mut matched: Option<EdgeId> = None;
        for (edge_id, edge) in tx.get_edges_from_node(src_id, true)? {
            if edge.dst != dst_id || edge.label_id != rel_label {
                continue;
            }
            let mut ok = true;
            for (key, value) in &wanted {
                let got = edge_property_value(tx, edge_id, key)?;
                if got != *value {
                    ok = false;
                    break;
                }
            }
            if ok {
                // If the rel variable is already bound, only reuse that edge.
                if let Some(rv) = rel_var
                    && let Some(Binding::Edge(bound)) = row.get(rv)
                    && *bound != edge_id
                {
                    continue;
                }
                matched = Some(edge_id);
                break;
            }
        }
        match matched {
            Some(edge_id) => {
                if let Some(rv) = rel_var {
                    row.insert(rv.clone(), Binding::Edge(edge_id));
                }
                apply_set_actions(&row, on_match, tx, params)?;
                out.push(row);
            }
            None => {
                let edge_id = tx.create_edge_with_label(src_id, dst_id, rel_label)?;
                for (key, value) in &wanted {
                    tx.set_edge_property(edge_id, key, value)?;
                }
                if let Some(rv) = rel_var {
                    row.insert(rv.clone(), Binding::Edge(edge_id));
                }
                apply_set_actions(&row, on_create, tx, params)?;
                out.push(row);
            }
        }
    }
    Ok(out)
}

fn apply_set_actions(
    row: &Row,
    actions: &[SetClause],
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<(), DbError> {
    for action in actions {
        let value = eval_expr(&action.value, row, tx, params)?;
        match row.get(&action.variable) {
            Some(Binding::Node(node_id)) => {
                check_unique_constraint_multi(tx, *node_id, &action.property, &value)?;
                tx.set_node_property(*node_id, &action.property, &value)?;
            }
            Some(Binding::Edge(edge_id)) => {
                tx.set_edge_property(*edge_id, &action.property, &value)?;
            }
            Some(Binding::Value(_)) | None => {
                return Err(DbError::QueryError(format!(
                    "ON CREATE/MATCH SET references unknown entity variable `{}`",
                    action.variable
                )));
            }
        }
    }
    Ok(())
}

/// Scans nodes (optionally using an index) and binds them to a variable.
/// When `optional` is true, input rows with no match are preserved unbound.
#[allow(clippy::too_many_arguments)]
fn scan_nodes(
    rows: Vec<Row>,
    variable: &str,
    label: &Option<String>,
    extra_labels: &[String],
    filter: &Option<Expression>,
    index_hint: &NodeIndexHint,
    optional: bool,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    let wanted_label_id = match label {
        Some(label) => Some(label_id_for(tx, Some(label))?),
        None => None,
    };
    let mut wanted_extra_ids = Vec::with_capacity(extra_labels.len());
    for extra in extra_labels {
        wanted_extra_ids.push(label_id_for(tx, Some(extra))?);
    }

    // Try to use an index when a matching hint is present.
    let indexed_ids: Option<Vec<NodeId>> = match index_hint {
        NodeIndexHint::Label { label } => tx.lookup_nodes_by_label(label)?,
        NodeIndexHint::Property { key, value } => tx.lookup_nodes_by_property(key, value)?,
        NodeIndexHint::LabelAndProperty { label, key, value } => {
            tx.lookup_nodes_by_label_and_property(label, key, value)?
        }
        NodeIndexHint::FullScan => None,
    };

    let nodes: Vec<(NodeId, NodeRecord)> = match indexed_ids {
        Some(ids) => ids
            .into_iter()
            .map(|id| tx.get_node(id).map(|node| (id, node)))
            .collect::<Result<Vec<_>, _>>()?,
        None => tx.scan_nodes()?,
    };

    let mut out = Vec::new();
    for row in rows {
        // If the variable is already bound (e.g. from an earlier MERGE/CREATE
        // or a repeated MATCH), only keep rows where the bound entity matches.
        if let Some(existing) = row.get(variable) {
            let mut matched = false;
            for (node_id, node) in &nodes {
                if !node_matches(node, wanted_label_id, &wanted_extra_ids) {
                    continue;
                }
                if !binding_matches(&row, variable, &Binding::Node(*node_id))? {
                    continue;
                }
                let _ = existing;
                let mut next = row.clone();
                next.insert(variable.to_string(), Binding::Node(*node_id));
                if filter
                    .as_ref()
                    .is_none_or(|expr| eval_truthy(expr, &next, tx, params).unwrap_or(false))
                {
                    matched = true;
                    out.push(next);
                }
            }
            if !matched && optional {
                out.push(row);
            }
            continue;
        }
        let mut matched_any = false;
        for (node_id, node) in &nodes {
            if !node_matches(node, wanted_label_id, &wanted_extra_ids) {
                continue;
            }
            if !binding_matches(&row, variable, &Binding::Node(*node_id))? {
                continue;
            }
            let mut next = row.clone();
            next.insert(variable.to_string(), Binding::Node(*node_id));
            if filter
                .as_ref()
                .is_none_or(|expr| eval_truthy(expr, &next, tx, params).unwrap_or(false))
            {
                matched_any = true;
                out.push(next);
            }
        }
        if !matched_any && optional {
            out.push(row);
        }
    }
    Ok(out)
}

fn node_matches(node: &NodeRecord, wanted_label_id: Option<u32>, wanted_extra_ids: &[u32]) -> bool {
    if let Some(label_id) = wanted_label_id
        && node.label_id != label_id
        && !node.extra_labels.contains(&label_id)
    {
        return false;
    }
    for extra in wanted_extra_ids {
        if !node.has_label(*extra) {
            return false;
        }
    }
    true
}

/// Traverses edges from each bound node, optionally filtered by type and direction.
/// Supports single-hop and variable-length (`hops.is_some()`) BFS expansion.
#[allow(clippy::too_many_arguments)]
fn traverse_edges(
    rows: Vec<Row>,
    from_var: &str,
    edge_type: &Option<String>,
    direction: &Direction,
    to_var: &str,
    to_label: &Option<String>,
    to_extra_labels: &[String],
    hops: &Option<crate::query::ast::RelationshipLength>,
    edge_var: &Option<String>,
    edge_filter: &Option<Expression>,
    optional: bool,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    if hops.is_some() {
        return traverse_edges_var_length(
            rows,
            from_var,
            edge_type,
            direction,
            to_var,
            to_label,
            to_extra_labels,
            hops.as_ref(),
            optional,
            tx,
            params,
        );
    }
    let edge_label = match edge_type {
        Some(label) => Some(label_id_for(tx, Some(label))?),
        None => None,
    };
    let node_label = match to_label {
        Some(label) => Some(label_id_for(tx, Some(label))?),
        None => None,
    };
    let mut extra_ids = Vec::with_capacity(to_extra_labels.len());
    for extra in to_extra_labels {
        extra_ids.push(label_id_for(tx, Some(extra))?);
    }
    let mut out = Vec::new();
    for row in rows {
        let Some(from_binding) = row.get(from_var) else {
            if optional {
                out.push(row);
                continue;
            }
            return Err(DbError::QueryError(format!(
                "unbound traversal variable `{}`",
                from_var
            )));
        };
        let Binding::Node(from_id) = from_binding else {
            return Err(DbError::QueryError(format!(
                "traversal variable `{}` is not a node",
                from_var
            )));
        };
        let from_id = *from_id;

        let outgoing = match direction {
            Direction::Outgoing | Direction::Undirected => true,
            Direction::Incoming => false,
        };
        let edges = tx.get_edges_from_node(from_id, outgoing)?;
        let mut matched_any = false;
        for (edge_id, edge) in &edges {
            if edge_label.is_some_and(|label_id| edge.label_id != label_id) {
                continue;
            }
            let candidate = match direction {
                Direction::Outgoing if edge.src == from_id => Some(edge.dst),
                Direction::Incoming if edge.dst == from_id => Some(edge.src),
                Direction::Undirected if edge.src == from_id => Some(edge.dst),
                Direction::Undirected if edge.dst == from_id => Some(edge.src),
                _ => None,
            };
            let Some(to_id) = candidate else { continue };
            let to_node = tx.get_node(to_id)?;
            if let Some(label_id) = node_label
                && !to_node.has_label(label_id)
            {
                continue;
            }
            let mut extras_ok = true;
            for extra in &extra_ids {
                if !to_node.has_label(*extra) {
                    extras_ok = false;
                    break;
                }
            }
            if !extras_ok {
                continue;
            }
            if !binding_matches(&row, to_var, &Binding::Node(to_id))? {
                continue;
            }
            if let Some(edge_var) = edge_var
                && !binding_matches(&row, edge_var, &Binding::Edge(*edge_id))?
            {
                continue;
            }
            let mut next = row.clone();
            next.insert(to_var.to_string(), Binding::Node(to_id));
            if let Some(edge_var) = edge_var {
                next.insert(edge_var.clone(), Binding::Edge(*edge_id));
            }
            if edge_filter
                .as_ref()
                .is_none_or(|expr| eval_truthy(expr, &next, tx, params).unwrap_or(false))
            {
                matched_any = true;
                out.push(next);
            }
        }
        if !matched_any && optional {
            out.push(row);
        }
    }
    Ok(out)
}

/// BFS variable-length traversal with cycle avoidance and hop guardrails.
#[allow(clippy::too_many_arguments)]
fn traverse_edges_var_length(
    rows: Vec<Row>,
    from_var: &str,
    edge_type: &Option<String>,
    direction: &Direction,
    to_var: &str,
    to_label: &Option<String>,
    to_extra_labels: &[String],
    hops: Option<&crate::query::ast::RelationshipLength>,
    optional: bool,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    let edge_label = match edge_type {
        Some(label) => Some(label_id_for(tx, Some(label))?),
        None => None,
    };
    let node_label = match to_label {
        Some(label) => Some(label_id_for(tx, Some(label))?),
        None => None,
    };
    let mut extra_ids = Vec::with_capacity(to_extra_labels.len());
    for extra in to_extra_labels {
        extra_ids.push(label_id_for(tx, Some(extra))?);
    }
    let (min_hops, max_hops) = match hops {
        Some(length) => {
            let min = length.min_hops.unwrap_or(1);
            let max = length.max_hops.unwrap_or(MAX_VAR_HOPS);
            (min, max.min(MAX_VAR_HOPS))
        }
        None => (1, MAX_VAR_HOPS),
    };
    if min_hops > max_hops {
        return Err(DbError::QueryError(
            "invalid traversal bounds: min exceeds max".to_string(),
        ));
    }
    let mut out = Vec::new();
    for row in rows {
        let Some(from_binding) = row.get(from_var) else {
            if optional {
                out.push(row);
                continue;
            }
            return Err(DbError::QueryError(format!(
                "unbound traversal variable `{}`",
                from_var
            )));
        };
        let Binding::Node(from_id) = from_binding else {
            return Err(DbError::QueryError(format!(
                "traversal variable `{}` is not a node",
                from_var
            )));
        };
        let from_id = *from_id;
        // BFS frontier: (node_id, depth). Visited prevents infinite cycles.
        let mut visited: HashSet<NodeId> = HashSet::new();
        visited.insert(from_id);
        let mut frontier = vec![from_id];
        let mut matched_any = false;
        // Collect nodes by depth so min_hops is respected.
        let mut depth = 0u32;
        // Depth 0 is the start node itself; expand outward.
        while !frontier.is_empty() && depth < max_hops {
            depth += 1;
            let mut next_frontier = Vec::new();
            for current in frontier {
                let outgoing_edges = match direction {
                    Direction::Outgoing | Direction::Undirected => {
                        tx.get_edges_from_node(current, true)?
                    }
                    Direction::Incoming => Vec::new(),
                };
                let incoming_edges = match direction {
                    Direction::Incoming | Direction::Undirected => {
                        tx.get_edges_from_node(current, false)?
                    }
                    Direction::Outgoing => Vec::new(),
                };
                for (edge_id, edge) in outgoing_edges.iter().chain(incoming_edges.iter()) {
                    if edge_label.is_some_and(|label_id| edge.label_id != label_id) {
                        continue;
                    }
                    let candidate = match direction {
                        Direction::Outgoing if edge.src == current => Some(edge.dst),
                        Direction::Incoming if edge.dst == current => Some(edge.src),
                        Direction::Undirected if edge.src == current => Some(edge.dst),
                        Direction::Undirected if edge.dst == current => Some(edge.src),
                        _ => None,
                    };
                    let Some(to_id) = candidate else { continue };
                    let _ = edge_id;
                    if !visited.insert(to_id) {
                        continue;
                    }
                    next_frontier.push(to_id);
                    if depth >= min_hops {
                        let to_node = match tx.get_node(to_id) {
                            Ok(n) => n,
                            Err(_) => continue,
                        };
                        if let Some(label_id) = node_label
                            && !to_node.has_label(label_id)
                        {
                            continue;
                        }
                        let mut extras_ok = true;
                        for extra in &extra_ids {
                            if !to_node.has_label(*extra) {
                                extras_ok = false;
                                break;
                            }
                        }
                        if !extras_ok {
                            continue;
                        }
                        if !binding_matches(&row, to_var, &Binding::Node(to_id))? {
                            continue;
                        }
                        let mut next = row.clone();
                        next.insert(to_var.to_string(), Binding::Node(to_id));
                        // Edge variables are not bound for variable-length
                        // traversals (multiple edges); they read as NULL.
                        let _ = params;
                        matched_any = true;
                        out.push(next);
                    }
                }
            }
            frontier = next_frontier;
        }
        if !matched_any && optional {
            out.push(row);
        }
    }
    Ok(out)
}

/// Filters rows to only those where the condition evaluates to `true`.
fn filter_rows(
    rows: Vec<Row>,
    condition: &Expression,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    rows.into_iter()
        .filter_map(|row| match eval_truthy(condition, &row, tx, params) {
            Ok(true) => Some(Ok(row)),
            Ok(false) => None,
            Err(err) => Some(Err(err)),
        })
        .collect()
}

/// Implements `WITH` projection: evaluates each item per row and replaces the
/// row scope with the projected aliases. Entity variables pass through as
/// entity bindings so later `MATCH` can traverse from them.
/// Implements `WITH` projection: evaluates each item per row and replaces the
/// row scope with the projected aliases. When any item is an aggregate, rows
/// are first grouped exactly like `RETURN` aggregation and each group yields
/// one row (entity variables then evaluate to value maps, since a group has
/// no single entity to pass through).
fn project_with(
    rows: &[Row],
    clause: &ReturnClause,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    let has_aggregation = clause.items.iter().any(|item| {
        contains_aggregation(&item.expression)
            || clause
                .order_by
                .iter()
                .any(|o| contains_aggregation(&o.expression))
    });
    if has_aggregation {
        return project_with_aggregate(rows, clause, tx, params);
    }
    let mut projected: Vec<(Row, Vec<Value>)> = Vec::with_capacity(rows.len());
    for row in rows {
        let mut next = Row::new();
        let mut order_values = Vec::with_capacity(clause.order_by.len());
        for item in &clause.items {
            let name = item
                .alias
                .clone()
                .unwrap_or_else(|| expression_name(&item.expression));
            // Pass entity bindings through by reference so traversal still works.
            match &item.expression {
                Expression::Variable(v) => match row.get(v) {
                    Some(b @ (Binding::Node(_) | Binding::Edge(_))) => {
                        next.insert(name, b.clone());
                    }
                    _ => {
                        let value = eval_expr(&item.expression, row, tx, params)?;
                        next.insert(name, Binding::Value(value));
                    }
                },
                _ => {
                    let value = eval_expr(&item.expression, row, tx, params)?;
                    // If the value is an entity map produced from a variable,
                    // keep the original binding when possible for traversal.
                    next.insert(name, Binding::Value(value));
                }
            }
        }
        for item in &clause.order_by {
            order_values.push(eval_expr(&item.expression, row, tx, params)?);
        }
        projected.push((next, order_values));
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
    Ok(projected
        .into_iter()
        .skip(skip)
        .take(limit)
        .map(|(row, _)| row)
        .collect())
}

/// Aggregating `WITH`: groups rows like `RETURN` aggregation and binds one
/// output row per group under the projected aliases.
fn project_with_aggregate(
    rows: &[Row],
    clause: &ReturnClause,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Row>, DbError> {
    let groups = group_rows_for_aggregation(rows, &clause.items, tx, params)?;
    let mut projected: Vec<(Row, Vec<Value>)> = Vec::with_capacity(groups.len());
    for group_rows in &groups {
        let mut next = Row::new();
        for item in &clause.items {
            let name = item
                .alias
                .clone()
                .unwrap_or_else(|| expression_name(&item.expression));
            let value = eval_aggregate_expr(&item.expression, group_rows, tx, params)?;
            next.insert(name, Binding::Value(value));
        }
        let mut order_values = Vec::with_capacity(clause.order_by.len());
        for item in &clause.order_by {
            if contains_aggregation(&item.expression) {
                order_values.push(eval_aggregate_expr(
                    &item.expression,
                    group_rows,
                    tx,
                    params,
                )?);
            } else {
                let first = group_rows.first().copied();
                match first {
                    Some(row) => order_values.push(eval_expr(&item.expression, row, tx, params)?),
                    None => order_values.push(Value::Null),
                }
            }
        }
        projected.push((next, order_values));
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
    Ok(projected
        .into_iter()
        .skip(skip)
        .take(limit)
        .map(|(row, _)| row)
        .collect())
}

/// Sets a property on each bound entity (node or edge) in the given variable across all rows.
fn set_properties(
    rows: &[Row],
    variable: &str,
    key: &str,
    value_expr: &Expression,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<(), DbError> {
    for row in rows {
        let value = eval_expr(value_expr, row, tx, params)?;
        match row.get(variable) {
            Some(Binding::Node(node_id)) => {
                check_unique_constraint_multi(tx, *node_id, key, &value)?;
                tx.set_node_property(*node_id, key, &value)?;
            }
            Some(Binding::Edge(edge_id)) => tx.set_edge_property(*edge_id, key, &value)?,
            Some(Binding::Value(_)) => {
                return Err(DbError::QueryError(format!(
                    "SET target `{}` is a projected value, not an entity",
                    variable
                )));
            }
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

/// Deletes the specified variables from all rows.  If `detach` is true, incident edges are also deleted.
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
                Some(Binding::Node(node_id)) => {
                    node_ids.insert(*node_id);
                }
                Some(Binding::Edge(edge_id)) => {
                    edge_ids.insert(*edge_id);
                }
                Some(Binding::Value(_)) => {
                    return Err(DbError::QueryError(format!(
                        "DELETE target `{}` is a projected value, not an entity",
                        variable
                    )));
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
        for node_id in &node_ids {
            for (edge_id, _) in tx.get_edges_from_node(*node_id, true)? {
                edge_ids.insert(edge_id);
            }
            for (edge_id, _) in tx.get_edges_from_node(*node_id, false)? {
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

/// Removes labels or properties per `REMOVE` items across all rows.
fn remove_entities(
    rows: &[Row],
    clause: &RemoveClause,
    tx: &mut Transaction<'_>,
) -> Result<(), DbError> {
    for row in rows {
        for item in &clause.items {
            match row.get(&item.variable) {
                Some(Binding::Node(node_id)) => {
                    let node_id = *node_id;
                    if let Some(label) = &item.label {
                        tx.remove_node_label(node_id, label)?;
                    } else if let Some(property) = &item.property {
                        tx.remove_node_property(node_id, property)?;
                    }
                }
                Some(Binding::Edge(edge_id)) => {
                    let edge_id = *edge_id;
                    if item.label.is_some() {
                        return Err(DbError::QueryError(
                            "REMOVE of a label from an edge is not supported".to_string(),
                        ));
                    }
                    if let Some(property) = &item.property {
                        tx.remove_edge_property(edge_id, property)?;
                    }
                }
                Some(Binding::Value(_)) => {
                    return Err(DbError::QueryError(format!(
                        "REMOVE target `{}` is a projected value, not an entity",
                        item.variable
                    )));
                }
                None => {
                    return Err(DbError::QueryError(format!(
                        "unknown variable `{}`",
                        item.variable
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Projects the final result set from the accumulated rows according to the RETURN clause.
/// Supports `COUNT(*)` / `COUNT(expr)` aggregation with group-by over
/// non-aggregate return expressions.
fn project_return(
    rows: &[Row],
    clause: &ReturnClause,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<QueryResult, DbError> {
    let has_aggregation = clause.items.iter().any(|item| {
        contains_aggregation(&item.expression)
            || clause
                .order_by
                .iter()
                .any(|o| contains_aggregation(&o.expression))
    });
    if has_aggregation {
        return project_return_aggregate(rows, clause, tx, params);
    }
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
            .map(|item| eval_expr(&item.expression, row, tx, params))
            .collect::<Result<Vec<_>, _>>()?;
        let order_values = clause
            .order_by
            .iter()
            .map(|item| eval_expr(&item.expression, row, tx, params))
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

fn contains_aggregation(expr: &Expression) -> bool {
    match expr {
        Expression::Count(_)
        | Expression::Sum(_)
        | Expression::Avg(_)
        | Expression::Min(_)
        | Expression::Max(_) => true,
        Expression::BinaryOp { left, right, .. } => {
            contains_aggregation(left) || contains_aggregation(right)
        }
        Expression::UnaryOp { expr, .. } => contains_aggregation(expr),
        _ => false,
    }
}

/// Grouped aggregation: non-aggregate expressions form group keys, `COUNT`
/// expressions are computed per group. Bare `COUNT` mixed with entity
/// variables groups by those variables.
fn project_return_aggregate(
    rows: &[Row],
    clause: &ReturnClause,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<QueryResult, DbError> {
    let columns: Vec<String> = clause
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            item.alias
                .clone()
                .unwrap_or_else(|| match &item.expression {
                    Expression::Count(target) => match &**target {
                        CountTarget::Star => "count(*)".to_string(),
                        CountTarget::Expr(_) => format!("count({})", idx),
                    },
                    other => expression_name(other),
                })
        })
        .collect();
    // Group rows by the evaluated non-aggregate expressions.
    let groups = group_rows_for_aggregation(rows, &clause.items, tx, params)?;
    let mut projected: Vec<(Vec<Value>, Vec<Value>)> = Vec::new();
    for group_rows in &groups {
        let mut values = Vec::with_capacity(clause.items.len());
        for item in &clause.items {
            values.push(eval_aggregate_expr(
                &item.expression,
                group_rows,
                tx,
                params,
            )?);
        }
        // ORDER BY on aggregates: evaluate against the first row with group
        // context for non-aggregates, aggregate evaluation for COUNT.
        let mut order_values = Vec::with_capacity(clause.order_by.len());
        for item in &clause.order_by {
            if contains_aggregation(&item.expression) {
                order_values.push(eval_aggregate_expr(
                    &item.expression,
                    group_rows,
                    tx,
                    params,
                )?);
            } else {
                let first = group_rows.first().copied();
                match first {
                    Some(row) => order_values.push(eval_expr(&item.expression, row, tx, params)?),
                    None => order_values.push(Value::Null),
                }
            }
        }
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

/// Groups rows for aggregation by the evaluated non-aggregate items.
///
/// Returns one entry per group in first-seen order. When every item is an
/// aggregate (no group keys), the whole input is a single group — including
/// empty input, which yields one empty group so ungrouped aggregates still
/// produce a row (`COUNT(*)` → 0, others → `NULL`). With group keys present,
/// empty input yields no groups.
fn group_rows_for_aggregation<'r>(
    rows: &'r [Row],
    items: &[ReturnItem],
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Vec<&'r Row>>, DbError> {
    let has_group_keys = items
        .iter()
        .any(|item| !contains_aggregation(&item.expression));
    if rows.is_empty() {
        if has_group_keys {
            return Ok(Vec::new());
        }
        return Ok(vec![Vec::new()]);
    }
    let mut groups: HashMap<String, Vec<&'r Row>> = HashMap::new();
    let mut group_order: Vec<String> = Vec::new();
    for row in rows {
        let mut key_parts = Vec::new();
        for item in items {
            if contains_aggregation(&item.expression) {
                continue;
            }
            let value = eval_expr(&item.expression, row, tx, params)?;
            key_parts.push(value_to_group_key(&value));
        }
        let key = key_parts.join("\u{1f}");
        if !groups.contains_key(&key) {
            group_order.push(key.clone());
            groups.insert(key.clone(), Vec::new());
        }
        groups.get_mut(&key).unwrap().push(row);
    }
    if groups.is_empty() {
        // All return items are aggregates: single group over all rows.
        return Ok(vec![rows.iter().collect()]);
    }
    Ok(group_order
        .iter()
        .map(|key| groups.remove(key).unwrap_or_default())
        .collect())
}

fn eval_aggregate_expr(
    expr: &Expression,
    group_rows: &[&Row],
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Value, DbError> {
    match expr {
        Expression::Count(target) => match &**target {
            CountTarget::Star => Ok(Value::Integer(group_rows.len() as i64)),
            CountTarget::Expr(inner) => {
                let mut count = 0i64;
                for row in group_rows {
                    let value = eval_expr(inner, row, tx, params)?;
                    if !matches!(value, Value::Null) {
                        count += 1;
                    }
                }
                Ok(Value::Integer(count))
            }
        },
        Expression::Sum(inner) => eval_sum_agg(group_rows, inner, tx, params),
        Expression::Avg(inner) => eval_avg_agg(group_rows, inner, tx, params),
        Expression::Min(inner) => {
            eval_min_max_agg(group_rows, inner, Ordering::Less, "MIN", tx, params)
        }
        Expression::Max(inner) => {
            eval_min_max_agg(group_rows, inner, Ordering::Greater, "MAX", tx, params)
        }
        // Non-aggregate expressions: evaluate against the first row.
        _ => match group_rows.first() {
            Some(row) => eval_expr(expr, row, tx, params),
            None => Ok(Value::Null),
        },
    }
}

/// Collects the non-`NULL` argument values of an aggregate for one group.
fn aggregate_arg_values(
    group_rows: &[&Row],
    inner: &Expression,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Vec<Value>, DbError> {
    let mut values = Vec::with_capacity(group_rows.len());
    for row in group_rows {
        let value = eval_expr(inner, row, tx, params)?;
        if !matches!(value, Value::Null) {
            values.push(value);
        }
    }
    Ok(values)
}

/// `SUM(expr)`: integers sum to `Integer`, any float widens the result to
/// `Float`. Empty/all-`NULL` groups yield `NULL`; non-numeric values and
/// integer overflow are errors.
fn eval_sum_agg(
    group_rows: &[&Row],
    inner: &Expression,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Value, DbError> {
    let values = aggregate_arg_values(group_rows, inner, tx, params)?;
    if values.is_empty() {
        return Ok(Value::Null);
    }
    let mut int_sum: i64 = 0;
    let mut float_sum: f64 = 0.0;
    let mut has_float = false;
    for value in &values {
        match value {
            Value::Integer(n) => {
                int_sum = int_sum
                    .checked_add(*n)
                    .ok_or_else(|| DbError::QueryError("SUM(...) integer overflow".to_string()))?;
            }
            Value::Float(f) => {
                has_float = true;
                float_sum += *f;
            }
            other => {
                return Err(DbError::QueryError(format!(
                    "SUM(...) requires numeric values, got: {other}"
                )));
            }
        }
    }
    if has_float {
        Ok(Value::Float(int_sum as f64 + float_sum))
    } else {
        Ok(Value::Integer(int_sum))
    }
}

/// `AVG(expr)`: always yields `Float`. Empty/all-`NULL` groups yield `NULL`;
/// non-numeric values are errors.
fn eval_avg_agg(
    group_rows: &[&Row],
    inner: &Expression,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Value, DbError> {
    let values = aggregate_arg_values(group_rows, inner, tx, params)?;
    if values.is_empty() {
        return Ok(Value::Null);
    }
    let mut total = 0.0;
    for value in &values {
        match value {
            Value::Integer(n) => total += *n as f64,
            Value::Float(f) => total += *f,
            other => {
                return Err(DbError::QueryError(format!(
                    "AVG(...) requires numeric values, got: {other}"
                )));
            }
        }
    }
    Ok(Value::Float(total / values.len() as f64))
}

/// `MIN(expr)` / `MAX(expr)` over numbers (mixed int/float allowed) or
/// strings — but not mixed kinds. Empty/all-`NULL` groups yield `NULL`.
fn eval_min_max_agg(
    group_rows: &[&Row],
    inner: &Expression,
    want: Ordering,
    name: &str,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Value, DbError> {
    let values = aggregate_arg_values(group_rows, inner, tx, params)?;
    // 0 = number, 1 = string. Mixed kinds are rejected.
    let mut kind: Option<u8> = None;
    let mut best: Option<&Value> = None;
    for value in &values {
        let value_kind = match value {
            Value::Integer(_) | Value::Float(_) => 0,
            Value::String(_) => 1,
            other => {
                return Err(DbError::QueryError(format!(
                    "{name}(...) requires numeric or string values of a single type, got: {other}"
                )));
            }
        };
        match kind {
            Some(k) if k != value_kind => {
                return Err(DbError::QueryError(format!(
                    "{name}(...) requires numeric or string values of a single type (mixed kinds)"
                )));
            }
            _ => kind = Some(value_kind),
        }
        let replace = match best {
            None => true,
            Some(current) => compare_values(value, current) == want,
        };
        if replace {
            best = Some(value);
        }
    }
    Ok(best.cloned().unwrap_or(Value::Null))
}

fn value_to_group_key(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Integer(n) => format!("i:{n}"),
        Value::Float(f) => format!("f:{}", f.to_bits()),
        Value::Boolean(b) => format!("b:{b}"),
        Value::String(s) => format!("s:{s}"),
        Value::Map(m) => {
            let mut parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}={}", value_to_group_key(v)))
                .collect();
            parts.sort();
            format!("m:{{{}}}", parts.join(","))
        }
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(value_to_group_key).collect();
            format!("l:[{}]", parts.join(","))
        }
    }
}

/// Finds the first node matching the given label and property constraints.
fn find_matching_node(
    label_id: u32,
    properties: &HashMap<String, Expression>,
    row: &Row,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Option<(NodeId, NodeRecord)>, DbError> {
    for (node_id, node) in tx.scan_nodes()? {
        if node.label_id != label_id && !node.extra_labels.contains(&label_id) {
            // Unlabeled merge (label 0) matches only unlabeled nodes.
            if label_id != 0 {
                continue;
            }
            if node.label_id != 0 {
                continue;
            }
        }
        let mut matches = true;
        for (key, expr) in properties {
            let wanted = eval_expr(expr, row, tx, params)?;
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

/// Evaluates an expression and returns `true` only if it produces `Value::Boolean(true)`.
fn eval_truthy(
    expr: &Expression,
    row: &Row,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<bool, DbError> {
    Ok(matches!(
        eval_expr(expr, row, tx, params)?,
        Value::Boolean(true)
    ))
}

/// Evaluates an expression against the current row bindings and returns a `Value`.
fn eval_expr(
    expr: &Expression,
    row: &Row,
    tx: &mut Transaction<'_>,
    params: &Params,
) -> Result<Value, DbError> {
    match expr {
        Expression::Null => Ok(Value::Null),
        Expression::Integer(n) => Ok(Value::Integer(*n)),
        Expression::Float(n) => Ok(Value::Float(*n)),
        Expression::String(s) => Ok(Value::String(s.clone())),
        Expression::Boolean(b) => Ok(Value::Boolean(*b)),
        Expression::Param(name) => params.get(name).cloned().ok_or(DbError::QueryError(format!(
            "missing query parameter `${}`",
            name
        ))),
        Expression::Count(_) => Err(DbError::QueryError(
            "COUNT(...) is only supported in RETURN / WITH projections".to_string(),
        )),
        Expression::Sum(_) => Err(DbError::QueryError(
            "SUM(...) is only supported in RETURN / WITH projections".to_string(),
        )),
        Expression::Avg(_) => Err(DbError::QueryError(
            "AVG(...) is only supported in RETURN / WITH projections".to_string(),
        )),
        Expression::Min(_) => Err(DbError::QueryError(
            "MIN(...) is only supported in RETURN / WITH projections".to_string(),
        )),
        Expression::Max(_) => Err(DbError::QueryError(
            "MAX(...) is only supported in RETURN / WITH projections".to_string(),
        )),
        Expression::Variable(variable) => match row.get(variable) {
            Some(Binding::Node(node_id)) => entity_to_map_for_node(tx, *node_id),
            Some(Binding::Edge(edge_id)) => entity_to_map_for_edge(tx, *edge_id),
            Some(Binding::Value(value)) => Ok(value.clone()),
            None => Ok(Value::Null),
        },
        Expression::Property { variable, property } => match row.get(variable) {
            Some(Binding::Node(node_id)) => node_property_value(tx, *node_id, property),
            Some(Binding::Edge(edge_id)) => edge_property_value(tx, *edge_id, property),
            Some(Binding::Value(Value::Map(map))) => {
                Ok(map.get(property).cloned().unwrap_or(Value::Null))
            }
            Some(Binding::Value(_)) => Ok(Value::Null),
            None => Ok(Value::Null),
        },
        Expression::BinaryOp { left, op, right } => {
            let left = eval_expr(left, row, tx, params)?;
            let right = eval_expr(right, row, tx, params)?;
            eval_binary(left, op, right)
        }
        Expression::UnaryOp { op, expr } => match op {
            UnaryOp::Not => Ok(Value::Boolean(!matches!(
                eval_expr(expr, row, tx, params)?,
                Value::Boolean(true)
            ))),
        },
    }
}

/// Evaluates a binary operation on two values and returns the result.
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

/// Compares two values and returns their ordering.  Supports cross-type numeric comparison.
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

/// Converts a node into a `Value::Map` with keys `id`, `label`, `labels`, and `properties`.
fn entity_to_map_for_node(tx: &mut Transaction<'_>, node_id: NodeId) -> Result<Value, DbError> {
    let node = tx.get_node(node_id)?;
    let label_name = if node.label_id != 0 {
        tx.get_label_name(node.label_id)?
            .unwrap_or_else(|| format!("label_{}", node.label_id))
    } else {
        String::new()
    };
    let mut all_labels = Vec::new();
    for lid in node.all_label_ids() {
        if let Some(name) = tx.get_label_name(lid)? {
            all_labels.push(Value::String(name));
        }
    }
    let props = tx.list_node_properties(node_id)?;
    let mut map = HashMap::new();
    map.insert("id".to_string(), Value::Integer(node.id as i64));
    map.insert("label".to_string(), Value::String(label_name));
    map.insert("labels".to_string(), Value::List(all_labels));
    let mut props_map = HashMap::new();
    for (k, v) in props {
        props_map.insert(k, v);
    }
    map.insert("properties".to_string(), Value::Map(props_map));
    Ok(Value::Map(map))
}

/// Converts an edge into a `Value::Map` with keys `id`, `type`, `src`, `dst`, and `properties`.
fn entity_to_map_for_edge(tx: &mut Transaction<'_>, edge_id: EdgeId) -> Result<Value, DbError> {
    let edge = tx.get_edge(edge_id)?;
    let type_name = if edge.label_id != 0 {
        tx.get_label_name(edge.label_id)?
            .unwrap_or_else(|| format!("type_{}", edge.label_id))
    } else {
        String::new()
    };
    let props = tx.list_edge_properties(edge_id)?;
    let mut map = HashMap::new();
    map.insert("id".to_string(), Value::Integer(edge.id as i64));
    map.insert("type".to_string(), Value::String(type_name));
    map.insert("src".to_string(), Value::Integer(edge.src as i64));
    map.insert("dst".to_string(), Value::Integer(edge.dst as i64));
    let mut props_map = HashMap::new();
    for (k, v) in props {
        props_map.insert(k, v);
    }
    map.insert("properties".to_string(), Value::Map(props_map));
    Ok(Value::Map(map))
}

/// Reads a node property by key name, returning `Value::Null` if the property does not exist.
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

/// Reads an edge property by key name, returning `Value::Null` if the property does not exist.
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

/// Returns `true` if the row's variable is either unbound or already matches the candidate.
fn binding_matches(row: &Row, variable: &str, candidate: &Binding) -> Result<bool, DbError> {
    Ok(row
        .get(variable)
        .is_none_or(|existing| existing == candidate))
}

/// Resolves the label name to a `label_id`, registering it if necessary.
/// Returns 0 for `None` (unlabeled).
fn label_id_for(tx: &mut Transaction<'_>, label: Option<&str>) -> Result<u32, DbError> {
    match label {
        Some(label) => tx.register_label(label),
        None => Ok(0),
    }
}

/// Checks a node property value against unique constraints on all labels of the node.
fn check_unique_constraint_multi(
    tx: &mut Transaction<'_>,
    node_id: NodeId,
    key: &str,
    value: &Value,
) -> Result<(), DbError> {
    let node = match tx.get_node(node_id) {
        Ok(n) => n,
        // Node was just created but label may not be set yet in some paths;
        // fall back to no-op when unreadable here (caller handles creation).
        Err(_) => return Ok(()),
    };
    let Some(key_id) = tx.find_property_key(key)? else {
        return Ok(());
    };
    for label_id in node.all_label_ids() {
        tx.check_unique_constraint(label_id, key_id, value, Some(node_id))?;
    }
    Ok(())
}

/// Generates a default column name from an expression.
fn expression_name(expr: &Expression) -> String {
    match expr {
        Expression::Variable(variable) => variable.clone(),
        Expression::Property { variable, property } => format!("{}.{}", variable, property),
        Expression::Param(name) => format!("${}", name),
        Expression::Count(target) => match &**target {
            CountTarget::Star => "count(*)".to_string(),
            CountTarget::Expr(_) => "count".to_string(),
        },
        Expression::Sum(_) => "sum".to_string(),
        Expression::Avg(_) => "avg".to_string(),
        Expression::Min(_) => "min".to_string(),
        Expression::Max(_) => "max".to_string(),
        Expression::Null => "null".to_string(),
        _ => "expr".to_string(),
    }
}

/// Decodes a `Value` from a `PropertyEntry`'s inline bytes (unused, kept for potential future use).
#[allow(dead_code)]
fn inline_property_value(entry: &PropertyEntry) -> Value {
    Value::from_bytes(entry.value_type, entry.value_inline)
}
