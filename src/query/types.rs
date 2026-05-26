use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

impl QueryResult {
    /// Creates a new `QueryResult` with the given column names and data rows.
    pub fn new(columns: Vec<String>, rows: Vec<Vec<Value>>) -> Self {
        QueryResult { columns, rows }
    }
}
