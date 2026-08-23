//! The value and column-type vocabulary shared by the parser, storage, and executor.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnType {
    Integer,
    Text,
    Real,
}

impl ColumnType {
    pub fn name(&self) -> &'static str {
        match self {
            ColumnType::Integer => "INTEGER",
            ColumnType::Text => "TEXT",
            ColumnType::Real => "REAL",
        }
    }
}

impl fmt::Display for ColumnType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Integer(i64),
    Real(f64),
    Text(String),
}

impl Value {
    pub fn type_of(&self) -> ColumnType {
        match self {
            Value::Integer(_) => ColumnType::Integer,
            Value::Real(_) => ColumnType::Real,
            Value::Text(_) => ColumnType::Text,
        }
    }

    /// Value equality/ordering across INTEGER and REAL is allowed (2 = 2.0),
    /// TEXT only compares to TEXT.
    pub fn partial_compare(&self, other: &Value) -> Option<std::cmp::Ordering> {
        use Value::*;
        match (self, other) {
            (Integer(a), Integer(b)) => a.partial_cmp(b),
            (Real(a), Real(b)) => a.partial_cmp(b),
            (Integer(a), Real(b)) => (*a as f64).partial_cmp(b),
            (Real(a), Integer(b)) => a.partial_cmp(&(*b as f64)),
            (Text(a), Text(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Integer(i) => write!(f, "{i}"),
            Value::Real(r) => write!(f, "{r}"),
            Value::Text(s) => write!(f, "{s}"),
        }
    }
}
