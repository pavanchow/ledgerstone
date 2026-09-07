<img src="docs/logo.svg" alt="Ledgerstone logo" width="96">

**Ledgerstone is an embedded relational database in Rust, small enough to read end to end.**

Ledgerstone is a from-scratch embedded relational database. It has real tables with typed columns, real rows persisted to disk, and a genuine hand-written SQL parser and executor, all in a single readable crate with no server process to run and no C library underneath. Where SQLite is a superb but large C engine, Ledgerstone is the version you can open in an editor and follow start to finish, storage, parsing, and execution, in a few thousand lines of Rust.

## SQL supported

- `CREATE TABLE t (col TYPE, ...)` with `INTEGER`, `TEXT`, and `REAL` columns
- `INSERT INTO t VALUES (...)` and `INSERT INTO t (cols) VALUES (...)`
- `SELECT col, col FROM t WHERE <expr> ORDER BY col [ASC|DESC] LIMIT n` and `SELECT * FROM t`
- `UPDATE t SET col = val, ... WHERE <expr>`
- `DELETE FROM t WHERE <expr>`
- WHERE expressions: `=`, `!=`, `<`, `>`, `<=`, `>=` on a column and a literal, combined with `AND`, `OR`, and parentheses

Bad SQL, wrong value types, and missing tables or columns all come back as typed errors. Nothing in the engine panics on malformed input.

## Usage

```
# open an interactive REPL against a database file (created if absent)
ledgerstone --db data.lst

# run one statement and exit
ledgerstone --db data.lst exec "SELECT * FROM users WHERE score > 8.0 ORDER BY score DESC"

# serve a local HTTP API: POST /query with raw SQL, get JSON back
ledgerstone --db data.lst serve --port 7878

# run as an MCP server over stdio, exposing a ledgerstone_query tool
ledgerstone --db data.lst mcp
```

State persists across runs. A database is one file, so `data.lst` is the whole thing, copy it and you have copied the database.

## Build and test

```
cargo build --release
cargo test
```

## Try it live

`docs/index.html` runs a full JavaScript port of the lexer, parser, and executor in the browser, no build step, no server, preloaded with sample data. Open it and run a query.

By Pavan Nallamothu.
