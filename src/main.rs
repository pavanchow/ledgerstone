use clap::{Parser, Subcommand};
use ledgerstone::{run_sql, Database, QueryResult};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

#[derive(Parser)]
#[command(
    name = "ledgerstone",
    version,
    about = "An embedded relational database in Rust: a hand-written SQL parser and executor over a typed, persistent table store."
)]
struct Cli {
    /// Path to the database file (created if absent).
    #[arg(long, default_value = "data.lst", global = true)]
    db: String,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run one SQL statement and exit.
    Exec {
        /// The SQL statement, e.g. "SELECT * FROM t".
        sql: String,
    },
    /// Serve the database over a small local HTTP API (POST /query).
    Serve {
        #[arg(long, default_value_t = 7878)]
        port: u16,
    },
    /// Run as an MCP server over stdio so an agent can query the database.
    Mcp,
}

fn main() {
    let cli = Cli::parse();
    let db = match Database::open(&cli.db) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("ledgerstone: {e}");
            std::process::exit(1);
        }
    };

    match cli.cmd {
        Some(Cmd::Exec { sql }) => {
            let mut db = db;
            run_and_print(&mut db, &sql);
        }
        Some(Cmd::Serve { port }) => {
            let db = Arc::new(Mutex::new(db));
            if let Err(e) = ledgerstone::server::serve(db, port) {
                eprintln!("ledgerstone: {e}");
                std::process::exit(1);
            }
        }
        Some(Cmd::Mcp) => {
            let db = Arc::new(Mutex::new(db));
            if let Err(e) = ledgerstone::mcp::serve_mcp(db) {
                eprintln!("ledgerstone: {e}");
                std::process::exit(1);
            }
        }
        None => repl(db),
    }
}

fn repl(mut db: Database) {
    println!("ledgerstone {} -- {}", env!("CARGO_PKG_VERSION"), db.path().display());
    println!("Enter SQL statements ending in a newline. .quit to exit.");
    let stdin = io::stdin();
    loop {
        print!("ledgerstone> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        let n = match stdin.read_line(&mut line) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("ledgerstone: read error: {e}");
                break;
            }
        };
        if n == 0 {
            println!();
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == ".quit" || trimmed == ".exit" {
            break;
        }
        run_and_print(&mut db, trimmed);
    }
}

fn run_and_print(db: &mut Database, sql: &str) {
    match run_sql(db, sql) {
        Ok(QueryResult::Ok) => println!("OK"),
        Ok(QueryResult::RowsAffected(n)) => println!("{n} row(s) affected"),
        Ok(QueryResult::Rows { columns, rows }) => print_table(&columns, &rows),
        Err(e) => eprintln!("error: {e}"),
    }
}

fn print_table(columns: &[String], rows: &[Vec<ledgerstone::types::Value>]) {
    if columns.is_empty() {
        println!("(0 columns)");
        return;
    }
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    let str_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| r.iter().map(|v| v.to_string()).collect())
        .collect();
    for row in &str_rows {
        for (i, cell) in row.iter().enumerate() {
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(cell.len());
            }
        }
    }
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{:<width$}", c, width = widths[i]))
        .collect();
    println!("{}", header.join(" | "));
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    println!("{}", sep.join("-+-"));
    for row in &str_rows {
        let cells: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<width$}", c, width = widths[i]))
            .collect();
        println!("{}", cells.join(" | "));
    }
    println!("({} row{})", rows.len(), if rows.len() == 1 { "" } else { "s" });
}
