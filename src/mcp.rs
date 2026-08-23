//! An MCP server over stdio, so an agent can run SQL against a Ledgerstone
//! database directly. One tool: `ledgerstone_query`.

use crate::{run_sql, Database, QueryResult};
use serde_json::{json, Value as Json};
use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

pub fn serve_mcp(db: Arc<Mutex<Database>>) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let req: Json = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = req.get("id").cloned().unwrap_or(Json::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let resp = match method {
            "initialize" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "ledgerstone", "version": env!("CARGO_PKG_VERSION") }
                }
            }),
            "tools/list" => json!({
                "jsonrpc": "2.0", "id": id,
                "result": { "tools": tool_specs() }
            }),
            "tools/call" => match call_tool(&db, &req) {
                Ok(text) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "content": [{ "type": "text", "text": text }] }
                }),
                Err(msg) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": { "content": [{ "type": "text", "text": msg }], "isError": true }
                }),
            },
            "notifications/initialized" | "" => continue,
            _ => json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": "method not found" }
            }),
        };
        stdout.write_all(serde_json::to_string(&resp).unwrap().as_bytes())?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

fn tool_specs() -> Json {
    json!([{
        "name": "ledgerstone_query",
        "description": "Run one SQL statement against the open Ledgerstone database and return the result as text.",
        "inputSchema": {
            "type": "object",
            "properties": { "sql": { "type": "string", "description": "A single SQL statement." } },
            "required": ["sql"]
        }
    }])
}

fn call_tool(db: &Arc<Mutex<Database>>, req: &Json) -> Result<String, String> {
    let params = req.get("params").ok_or("missing params")?;
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    if name != "ledgerstone_query" {
        return Err(format!("unknown tool '{name}'"));
    }
    let sql = params
        .get("arguments")
        .and_then(|a| a.get("sql"))
        .and_then(|s| s.as_str())
        .ok_or("missing 'sql' argument")?;

    let mut db = db.lock().map_err(|_| "database lock poisoned".to_string())?;
    match run_sql(&mut db, sql) {
        Ok(QueryResult::Ok) => Ok("OK".to_string()),
        Ok(QueryResult::RowsAffected(n)) => Ok(format!("{n} row(s) affected")),
        Ok(QueryResult::Rows { columns, rows }) => Ok(format_rows(&columns, &rows)),
        Err(e) => Err(e.to_string()),
    }
}

fn format_rows(columns: &[String], rows: &[Vec<crate::types::Value>]) -> String {
    let mut out = columns.join(" | ");
    out.push('\n');
    for row in rows {
        let cells: Vec<String> = row.iter().map(|v| v.to_string()).collect();
        out.push_str(&cells.join(" | "));
        out.push('\n');
    }
    if rows.is_empty() {
        out.push_str("(0 rows)\n");
    }
    out
}
