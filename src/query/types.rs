use std::fmt::{Display, Formatter, Result as FmtResult};

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

    pub fn value_to_string(v: &Value) -> String {
        match v {
            Value::Null => "NULL".to_string(),
            Value::Integer(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Boolean(b) => b.to_string(),
            Value::String(s) => s.clone(),
        }
    }

    pub fn to_ascii_table(&self) -> String {
        if self.columns.is_empty() {
            return "(no columns)".to_string();
        }
        // Convert all cells to strings once
        let string_rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .map(|row| row.iter().map(Self::value_to_string).collect())
            .collect();
        // Compute width per column
        let mut widths: Vec<usize> = self.columns.iter().map(|c| c.len()).collect();
        for row in &string_rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() && cell.len() > widths[i] {
                    widths[i] = cell.len();
                }
            }
        }
        let border = {
            let mut s = String::new();
            s.push('+');
            for w in &widths {
                s.push_str(&"-".repeat(*w + 2));
                s.push('+');
            }
            s
        };
        let header = {
            let mut s = String::new();
            s.push('|');
            for (i, col) in self.columns.iter().enumerate() {
                s.push(' ');
                s.push_str(col);
                s.push_str(&" ".repeat(widths[i] - col.len() + 1));
                s.push('|');
            }
            s
        };
        let mut out = String::new();
        out.push_str(&border);
        out.push('\n');
        out.push_str(&header);
        out.push('\n');
        out.push_str(&border);
        for row in &string_rows {
            out.push('\n');
            out.push('|');
            for i in 0..widths.len() {
                let cell = row.get(i).cloned().unwrap_or_else(|| "NULL".to_string());
                out.push(' ');
                out.push_str(&cell);
                out.push_str(&" ".repeat(widths[i] - cell.len() + 1));
                out.push('|');
            }
        }
        out.push('\n');
        out.push_str(&border);
        out
    }
}

impl Display for QueryResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}", self.to_ascii_table())
    }
}
