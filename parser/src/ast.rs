use std::collections::HashMap;

/// A full parsed query.
///
/// Hive uses a clause pipeline so one query can flow through multiple operations,
/// for example `MATCH ... WHERE ... SET ... RETURN ...`.
#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    /// Ordered clauses exactly as they appear in the query.
    pub clauses: Vec<Clause>,
}

/// One top-level query clause.
///
/// The planner consumes these in order and turns each clause into executable plan
/// steps over a row/binding stream.
#[derive(Debug, Clone, PartialEq)]
pub enum Clause {
    /// `CREATE (...)` or `CREATE (...)-[...]-(...)`.
    Create(Pattern),
    /// `MATCH ...` graph pattern binding.
    Match(MatchClause),
    /// `WHERE ...` filter expression applied to the current row stream.
    Where(Expression),
    /// `SET n.key = expr` property mutation.
    Set(SetClause),
    /// `DELETE n` or `DETACH DELETE n` entity deletion.
    Delete(DeleteClause),
    /// `MERGE ...` match-or-create pattern.
    Merge(Pattern),
    /// `RETURN ...` projection, ordering, and row slicing.
    Return(ReturnClause),
}

/// Data carried by a `MATCH` clause.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchClause {
    /// Node or path pattern to bind into variables.
    pub pattern: Pattern,
}

/// Data carried by `SET variable.property = value`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetClause {
    /// Bound variable whose property should be written.
    pub variable: String,
    /// Property key to update on the bound node or edge.
    pub property: String,
    /// Expression evaluated per row to produce the new value.
    pub value: Expression,
}

/// Data carried by `DELETE` or `DETACH DELETE`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteClause {
    /// Variables to delete. Multiple variables come from syntax like `DELETE n, r`.
    pub variables: Vec<String>,
    /// Whether incident relationships should be deleted before deleting nodes.
    pub detach: bool,
}

/// A graph pattern can be a single node or a path with relationships.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Single node pattern, for example `(n:Person {name: "Alice"})`.
    Node(NodePattern),
    /// Path pattern, for example `(a)-[r:KNOWS]->(b)`.
    Path(PathPattern),
}

/// Path pattern with one start node and one or more relationship segments.
#[derive(Debug, Clone, PartialEq)]
pub struct PathPattern {
    /// First node in the path.
    pub start: NodePattern,
    /// Relationship-plus-node segments after the start node.
    pub segments: Vec<PathSegment>,
}

/// One relationship hop and the node reached by that hop.
#[derive(Debug, Clone, PartialEq)]
pub struct PathSegment {
    /// Relationship pattern between the previous node and `node`.
    pub relationship: RelationshipPattern,
    /// Destination node for this segment.
    pub node: NodePattern,
}

/// Parsed node pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePattern {
    /// Optional variable name, for example `n` in `(n:Person)`.
    pub variable: Option<String>,
    /// Optional label name, for example `Person` in `(n:Person)`.
    pub label: Option<String>,
    /// Inline property predicates or create values from `{key: expr}`.
    pub properties: HashMap<String, Expression>,
}

/// Parsed relationship pattern.
#[derive(Debug, Clone, PartialEq)]
pub struct RelationshipPattern {
    /// Optional relationship variable, for example `r` in `[r:KNOWS]`.
    pub variable: Option<String>,
    /// Optional relationship type, for example `KNOWS` in `[:KNOWS]`.
    pub rel_type: Option<String>,
    /// Direction written in the pattern.
    pub direction: Direction,
    /// Optional variable-length traversal bounds from `*`, `*1..3`, etc.
    pub hops: Option<RelationshipLength>,
    /// Inline relationship property predicates or create values.
    pub properties: HashMap<String, Expression>,
}

/// Direction of a relationship pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    /// `-[]->` from left node to right node.
    Outgoing,
    /// `<-[]-` from right node to left node.
    Incoming,
    /// `-[]-` can match either direction.
    Undirected,
}

/// Data carried by a `RETURN` clause.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnClause {
    /// Expressions projected as result columns.
    pub items: Vec<ReturnItem>,
    /// Sort keys from `ORDER BY`.
    pub order_by: Vec<OrderItem>,
    /// Number of rows to skip before returning results.
    pub skip: Option<usize>,
    /// Maximum number of rows to return.
    pub limit: Option<usize>,
}

/// One `ORDER BY` expression.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem {
    /// Expression used as the sort key.
    pub expression: Expression,
    /// `true` for `DESC`, `false` for default/`ASC`.
    pub descending: bool,
}

/// One returned result column.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnItem {
    /// Expression to project into the result row.
    pub expression: Expression,
    /// Optional alias from `AS name`.
    pub alias: Option<String>,
}

/// Expression tree used by filters, properties, return items, and sort keys.
#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    /// Integer literal, for example `30`.
    Integer(i64),
    /// Floating-point literal, for example `3.14`.
    Float(f64),
    /// String literal, for example `"Alice"`.
    String(String),
    /// Boolean literal: `true` or `false`.
    Boolean(bool),
    /// Variable reference, for example `n`.
    Variable(String),
    /// Property access, for example `n.name`.
    Property {
        /// Variable that should already be bound in the row.
        variable: String,
        /// Property key to read from that entity.
        property: String,
    },
    /// Binary expression such as `a.age >= 30` or `x AND y`.
    BinaryOp {
        /// Left-hand expression.
        left: Box<Expression>,
        /// Operator applied between `left` and `right`.
        op: BinaryOp,
        /// Right-hand expression.
        right: Box<Expression>,
    },
    /// Unary expression such as `NOT active`.
    UnaryOp {
        /// Unary operator.
        op: UnaryOp,
        /// Expression the operator applies to.
        expr: Box<Expression>,
    },
}

/// Binary operators supported by query expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    /// `=` equality.
    Eq,
    /// `<>` inequality.
    Neq,
    /// `>` greater-than.
    Gt,
    /// `>=` greater-than-or-equal.
    Gte,
    /// `<` less-than.
    Lt,
    /// `<=` less-than-or-equal.
    Lte,
    /// Boolean `AND`.
    And,
    /// Boolean `OR`.
    Or,
}

/// Unary operators supported by query expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    /// Boolean negation.
    Not,
}

/// Bounds for variable-length relationship patterns.
#[derive(Debug, Clone, PartialEq)]
pub struct RelationshipLength {
    /// Minimum hops. `None` means unbounded from the lower side.
    pub min_hops: Option<u32>,
    /// Maximum hops. `None` means unbounded from the upper side.
    pub max_hops: Option<u32>,
}
