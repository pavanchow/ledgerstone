//! Ledgerstone: an embedded relational database in Rust.
//!
//! `storage` is the typed, persistent table store. `lexer` and `parser`
//! turn SQL text into a `Statement`. `executor` runs a `Statement` against
//! a `Database`. `run_sql` wires the three together for callers that just
//! want to hand in a string and get a result back.

pub mod error;
pub mod executor;
pub mod lexer;
pub mod mcp;
pub mod parser;
pub mod server;
pub mod storage;
pub mod types;

pub use error::{LsError, LsResult};
pub use executor::QueryResult;
pub use storage::Database;

/// Lexes, parses, and executes one SQL statement against `db`.
pub fn run_sql(db: &mut Database, sql: &str) -> LsResult<QueryResult> {
    let tokens = lexer::lex(sql)?;
    let stmt = parser::parse(tokens)?;
    executor::execute(db, stmt)
}
