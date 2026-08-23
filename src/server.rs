//! A minimal HTTP API, std-only, no framework. One route:
//!   POST /query   body = raw SQL text   ->  JSON { ok, columns, rows } or { ok: false, error }
//! Meant for local tooling, not for exposing on a public network.

use crate::{run_sql, Database, QueryResult};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A client that goes silent must not hold the server. Reads time out.
const READ_TIMEOUT: Duration = Duration::from_secs(15);
/// Cap the request body so a huge Content-Length cannot exhaust memory.
const MAX_BODY: usize = 16 * 1024 * 1024;

pub fn serve(db: Arc<Mutex<Database>>, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    eprintln!("ledgerstone serving on http://127.0.0.1:{port}");
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(READ_TIMEOUT));
                let db = Arc::clone(&db);
                if let Err(e) = handle(s, db) {
                    eprintln!("ledgerstone: connection error: {e}");
                }
            }
            Err(e) => eprintln!("ledgerstone: accept error: {e}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, db: Arc<Mutex<Database>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut method = "";
    let mut path = "";
    let mut parts = request_line.split_whitespace();
    if let (Some(m), Some(p)) = (parts.next(), parts.next()) {
        method = m;
        path = p;
    }

    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(v) = line.strip_prefix("Content-Length:").or_else(|| line.strip_prefix("content-length:")) {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }

    if content_length > MAX_BODY {
        return write_response(&mut stream, 413, &json!({"ok": false, "error": "request body too large"}));
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    if method != "POST" || path != "/query" {
        return write_response(&mut stream, 404, &json!({"ok": false, "error": "POST /query only"}));
    }

    let sql = String::from_utf8_lossy(&body).to_string();
    let result = {
        let mut db = db.lock().unwrap_or_else(|e| e.into_inner());
        run_sql(&mut db, &sql)
    };

    let body = match result {
        Ok(QueryResult::Ok) => json!({"ok": true, "columns": [], "rows": []}),
        Ok(QueryResult::RowsAffected(n)) => json!({"ok": true, "rows_affected": n}),
        Ok(QueryResult::Rows { columns, rows }) => {
            let rows_json: Vec<Vec<serde_json::Value>> = rows
                .iter()
                .map(|r| r.iter().map(value_to_json).collect())
                .collect();
            json!({"ok": true, "columns": columns, "rows": rows_json})
        }
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    };

    write_response(&mut stream, 200, &body)
}

fn value_to_json(v: &crate::types::Value) -> serde_json::Value {
    match v {
        crate::types::Value::Integer(i) => json!(i),
        crate::types::Value::Real(r) => json!(r),
        crate::types::Value::Text(s) => json!(s),
    }
}

fn write_response(stream: &mut TcpStream, status: u16, body: &serde_json::Value) -> std::io::Result<()> {
    let text = status_text(status);
    let payload = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    let header = format!(
        "HTTP/1.1 {status} {text}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&payload)?;
    stream.flush()
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Error",
    }
}
