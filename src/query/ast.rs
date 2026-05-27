use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Create(Pattern),
    Match(Box<MatchClause>),
    Delete(String),
    Set(Box<SetClause>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchClause {
    pub pattern: Pattern,
    pub where_clause: Option<WhereClause>,
    pub return_clause: ReturnClause,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetClause {
    pub variable: String,
    pub property: String,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Node(NodePattern),
    Edge(Box<NodePattern>, Box<RelationshipPattern>, Box<NodePattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct NodePattern {
    pub variable: Option<String>,
    pub label: Option<String>,
    pub properties: HashMap<String, Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelationshipPattern {
    pub variable: Option<String>,
    pub rel_type: Option<String>,
    pub direction: Direction,
    pub hops: Option<RelationshipLength>,
    pub properties: HashMap<String, Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Undirected,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WhereClause {
    pub condition: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnClause {
    pub items: Vec<ReturnItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReturnItem {
    pub expression: Expression,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    Variable(String),
    Property {
        variable: String,
        property: String,
    },
    BinaryOp {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RelationshipLength {
    pub min_hops: Option<u32>,
    pub max_hops: Option<u32>,
}
