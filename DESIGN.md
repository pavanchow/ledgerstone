# Ledgerstone design

## Storage format

A Ledgerstone database is a single file: an append-only log of operation records. There is no page cache and no B-tree. The format is a sequence of records, each written as

```
[4-byte little-endian length][JSON body of that many bytes]
```

The JSON body is one of four record kinds:

```
CreateTable { table, columns: [{ name, ty }] }
Insert      { table, row_id, values }
Update      { table, row_id, values }
Delete      { table, row_id }
```

Opening a database means reading the log front to back and replaying every record into memory, the same idea as replaying a write-ahead log after a restart. `CreateTable` creates an empty table with its schema. `Insert` adds a row under a row id that is never reused. `Update` and `Delete` look up an existing row id and change or remove it. Once replay finishes, the log is opened in append mode and every further statement appends one more record, flushed and fsynced before the call returns, so a completed write survives a crash.

Each table keeps its live rows in a `HashMap<row_id, Vec<Value>>` in memory. That map is the "index" every query goes through, the log on disk is write-only once a database is open, no statement re-reads the file.

Row ids are `u64`, assigned in order starting at 1 per table, and never reused, even across deletes, so a stale reference to a deleted row id cannot alias a later row.

## The SQL grammar

Ledgerstone implements a fixed subset of SQL, not a general parser. The grammar, roughly, in EBNF:

```
statement    := create_table | insert | select | delete | update

create_table := "CREATE" "TABLE" ident "(" column_def ("," column_def)* ")"
column_def   := ident ("INTEGER" | "TEXT" | "REAL")

insert       := "INSERT" "INTO" ident ["(" ident ("," ident)* ")"]
                "VALUES" "(" literal ("," literal)* ")"

select       := "SELECT" ("*" | ident ("," ident)*)
                "FROM" ident
                ["WHERE" expr]
                ["ORDER" "BY" ident ["ASC" | "DESC"]]
                ["LIMIT" int_literal]

delete       := "DELETE" "FROM" ident ["WHERE" expr]

update       := "UPDATE" ident "SET" ident "=" literal ("," ident "=" literal)*
                ["WHERE" expr]

expr         := and_expr ("OR" and_expr)*
and_expr     := primary ("AND" primary)*
primary      := "(" expr ")" | comparison
comparison   := ident compare_op literal
compare_op   := "=" | "!=" | "<>" | "<" | ">" | "<=" | ">="
literal      := int_literal | real_literal | string_literal
```

A comparison is always `column operator literal`, there is no column-to-column comparison and no subquery, by design, this is a subset meant to be read in one sitting.

### Lexer

The lexer (`src/lexer.rs`) is a single pass over the input `char` by `char`. It recognizes keywords case-insensitively, identifiers, integer and real literals, single-quoted string literals with `''` as an escaped quote, `--` line comments, and the operator set above. Anything it does not recognize becomes a `LsError::Syntax` immediately, the lexer never panics. A token-count cap (`MAX_TOKENS`) rejects pathologically long input before it ever reaches the parser.

### Parser

The parser (`src/parser.rs`) is hand-written recursive descent, one function per grammar rule. The only rule that can recurse on the shape of the input is the WHERE expression, through parenthesized grouping, `primary := "(" expr ")" | comparison`. Every entry into `parse_expr` and `parse_and` increments a depth counter, and once that counter passes `MAX_EXPR_DEPTH` (64), parsing fails with a syntax error instead of growing the native call stack further. A string of thousands of open parens returns an error in microseconds rather than crashing the process.

## The executor

The executor (`src/executor.rs`) takes a parsed `Statement` and a mutable `Database` and runs it:

- `CreateTable` fails with `LsError::Duplicate` if the table already exists, otherwise appends a `CreateTable` record and adds the table to memory.
- `Insert` resolves an explicit or positional column list, checks each value's runtime type against the column's declared type (`LsError::Type` on a mismatch or a wrong arity), assigns the next row id, appends an `Insert` record, and updates the in-memory map.
- `Select` scans the target table's row map, keeps the rows where the optional WHERE expression evaluates true, projects the requested columns (or all of them for `*`), sorts by the optional ORDER BY column, applies the optional LIMIT, and returns the result as a column list plus row list. WHERE evaluation walks the expression tree directly, `Compare` looks up the column index and compares against the literal, `And`/`Or` recurse into both sides.
- `Update` resolves and type-checks every assignment up front, finds the matching row ids, then rewrites each matching row's cells and appends one `Update` record per row.
- `Delete` finds the matching row ids and appends one `Delete` record per row, removing each from memory.

Comparisons between `INTEGER` and `REAL` are allowed (`2 = 2.0` is true), comparisons involving `TEXT` only succeed against another `TEXT`. Every error the executor can produce, a missing table, a missing column, a type mismatch, an existing table, is a variant of the typed `LsError` enum defined in `src/error.rs`, there is no `unwrap()` or `panic!()` on a bad-input path anywhere in the library.

## Interfaces

- **CLI** (`src/main.rs`): a REPL when no subcommand is given, `exec "<sql>"` to run one statement and exit, backed by `clap`.
- **HTTP API** (`src/server.rs`): a small `std`-only server, no framework, `POST /query` with a raw SQL body returns a JSON result.
- **MCP server** (`src/mcp.rs`): a stdio JSON-RPC server exposing one tool, `ledgerstone_query`, so an agent can run SQL against an open database.

All three call the same `run_sql(&mut Database, &str) -> LsResult<QueryResult>` entry point in `src/lib.rs`, which lexes, parses, and executes one statement, so the engine behaves identically no matter which interface is driving it.
