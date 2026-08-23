//! Typed errors. Nothing in this crate panics on bad SQL or bad input;
//! every failure path returns an `LsError` instead.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum LsError {
    /// The lexer or parser could not make sense of the input.
    Syntax(String),
    /// A statement referenced a table or column that does not exist.
    NotFound(String),
    /// A CREATE TABLE named a table that already exists.
    Duplicate(String),
    /// A value did not match its column's declared type, or arity was wrong.
    Type(String),
    /// The database file on disk could not be read or was not valid Ledgerstone data.
    Storage(String),
}

impl fmt::Display for LsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LsError::Syntax(m) => write!(f, "syntax error: {m}"),
            LsError::NotFound(m) => write!(f, "not found: {m}"),
            LsError::Duplicate(m) => write!(f, "already exists: {m}"),
            LsError::Type(m) => write!(f, "type error: {m}"),
            LsError::Storage(m) => write!(f, "storage error: {m}"),
        }
    }
}

impl std::error::Error for LsError {}

pub type LsResult<T> = Result<T, LsError>;
