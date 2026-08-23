use ledgerstone::types::Value;
use ledgerstone::{run_sql, Database, LsError, QueryResult};
use std::path::PathBuf;

fn temp_db_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("ledgerstone_test_{name}_{}.lst", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

fn exec(db: &mut Database, sql: &str) -> QueryResult {
    run_sql(db, sql).unwrap_or_else(|e| panic!("query failed: {sql}: {e}"))
}

#[test]
fn create_insert_select_returns_rows() {
    let path = temp_db_path("basic");
    let mut db = Database::open(&path).unwrap();

    exec(&mut db, "CREATE TABLE users (id INTEGER, name TEXT, score REAL)");
    exec(&mut db, "INSERT INTO users VALUES (1, 'alice', 9.5)");
    exec(&mut db, "INSERT INTO users VALUES (2, 'bob', 7.25)");
    exec(&mut db, "INSERT INTO users VALUES (3, 'carol', 8.0)");

    let result = exec(&mut db, "SELECT * FROM users WHERE score > 7.5");
    match result {
        QueryResult::Rows { columns, rows } => {
            assert_eq!(columns, vec!["id", "name", "score"]);
            assert_eq!(rows.len(), 2);
            let names: Vec<String> = rows
                .iter()
                .map(|r| match &r[1] {
                    Value::Text(s) => s.clone(),
                    _ => panic!("expected text"),
                })
                .collect();
            assert!(names.contains(&"alice".to_string()));
            assert!(names.contains(&"carol".to_string()));
        }
        other => panic!("expected rows, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn select_where_and_order_by_returns_right_order() {
    let path = temp_db_path("order");
    let mut db = Database::open(&path).unwrap();

    exec(&mut db, "CREATE TABLE t (id INTEGER, n INTEGER)");
    exec(&mut db, "INSERT INTO t VALUES (1, 30)");
    exec(&mut db, "INSERT INTO t VALUES (2, 10)");
    exec(&mut db, "INSERT INTO t VALUES (3, 20)");
    exec(&mut db, "INSERT INTO t VALUES (4, 40)");

    let result = exec(&mut db, "SELECT id FROM t WHERE n >= 20 ORDER BY n DESC");
    match result {
        QueryResult::Rows { rows, .. } => {
            let ids: Vec<i64> = rows
                .iter()
                .map(|r| match &r[0] {
                    Value::Integer(i) => *i,
                    _ => panic!("expected integer"),
                })
                .collect();
            assert_eq!(ids, vec![4, 1, 3]);
        }
        other => panic!("expected rows, got {other:?}"),
    }

    let result = exec(&mut db, "SELECT id FROM t ORDER BY n ASC LIMIT 2");
    match result {
        QueryResult::Rows { rows, .. } => {
            let ids: Vec<i64> = rows
                .iter()
                .map(|r| match &r[0] {
                    Value::Integer(i) => *i,
                    _ => panic!("expected integer"),
                })
                .collect();
            assert_eq!(ids, vec![2, 3]);
        }
        other => panic!("expected rows, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn where_and_or_combine_correctly() {
    let path = temp_db_path("and_or");
    let mut db = Database::open(&path).unwrap();

    exec(&mut db, "CREATE TABLE t (id INTEGER, a INTEGER, b TEXT)");
    exec(&mut db, "INSERT INTO t VALUES (1, 5, 'x')");
    exec(&mut db, "INSERT INTO t VALUES (2, 5, 'y')");
    exec(&mut db, "INSERT INTO t VALUES (3, 9, 'x')");
    exec(&mut db, "INSERT INTO t VALUES (4, 1, 'z')");

    let result = exec(&mut db, "SELECT id FROM t WHERE a = 5 AND b = 'x'");
    match result {
        QueryResult::Rows { rows, .. } => assert_eq!(rows.len(), 1),
        other => panic!("expected rows, got {other:?}"),
    }

    let result = exec(&mut db, "SELECT id FROM t WHERE a = 9 OR b = 'z'");
    match result {
        QueryResult::Rows { rows, .. } => assert_eq!(rows.len(), 2),
        other => panic!("expected rows, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn update_changes_only_matching_rows() {
    let path = temp_db_path("update");
    let mut db = Database::open(&path).unwrap();

    exec(&mut db, "CREATE TABLE t (id INTEGER, status TEXT)");
    exec(&mut db, "INSERT INTO t VALUES (1, 'pending')");
    exec(&mut db, "INSERT INTO t VALUES (2, 'pending')");
    exec(&mut db, "INSERT INTO t VALUES (3, 'done')");

    let result = exec(&mut db, "UPDATE t SET status = 'done' WHERE id = 2");
    assert_eq!(result, QueryResult::RowsAffected(1));

    let result = exec(&mut db, "SELECT id FROM t WHERE status = 'done' ORDER BY id ASC");
    match result {
        QueryResult::Rows { rows, .. } => {
            let ids: Vec<i64> = rows
                .iter()
                .map(|r| match &r[0] {
                    Value::Integer(i) => *i,
                    _ => panic!("expected integer"),
                })
                .collect();
            assert_eq!(ids, vec![2, 3]);
        }
        other => panic!("expected rows, got {other:?}"),
    }

    let result = exec(&mut db, "SELECT id FROM t WHERE status = 'pending'");
    match result {
        QueryResult::Rows { rows, .. } => assert_eq!(rows.len(), 1),
        other => panic!("expected rows, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn delete_removes_only_matching_rows() {
    let path = temp_db_path("delete");
    let mut db = Database::open(&path).unwrap();

    exec(&mut db, "CREATE TABLE t (id INTEGER, n INTEGER)");
    exec(&mut db, "INSERT INTO t VALUES (1, 10)");
    exec(&mut db, "INSERT INTO t VALUES (2, 20)");
    exec(&mut db, "INSERT INTO t VALUES (3, 30)");

    let result = exec(&mut db, "DELETE FROM t WHERE n >= 20");
    assert_eq!(result, QueryResult::RowsAffected(2));

    let result = exec(&mut db, "SELECT id FROM t");
    match result {
        QueryResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], Value::Integer(1));
        }
        other => panic!("expected rows, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn types_are_enforced_on_insert_and_update() {
    let path = temp_db_path("types");
    let mut db = Database::open(&path).unwrap();

    exec(&mut db, "CREATE TABLE t (id INTEGER, name TEXT)");

    let err = run_sql(&mut db, "INSERT INTO t VALUES ('not-an-int', 'x')").unwrap_err();
    assert!(matches!(err, LsError::Type(_)));

    let err = run_sql(&mut db, "INSERT INTO t VALUES (1, 'x', 'extra')").unwrap_err();
    assert!(matches!(err, LsError::Type(_)));

    exec(&mut db, "INSERT INTO t VALUES (1, 'x')");
    let err = run_sql(&mut db, "UPDATE t SET id = 'nope' WHERE id = 1").unwrap_err();
    assert!(matches!(err, LsError::Type(_)));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn persistence_survives_reopen() {
    let path = temp_db_path("persist");
    {
        let mut db = Database::open(&path).unwrap();
        exec(&mut db, "CREATE TABLE t (id INTEGER, name TEXT)");
        exec(&mut db, "INSERT INTO t VALUES (1, 'alice')");
        exec(&mut db, "INSERT INTO t VALUES (2, 'bob')");
        exec(&mut db, "DELETE FROM t WHERE id = 1");
        exec(&mut db, "UPDATE t SET name = 'bobby' WHERE id = 2");
    }

    let mut db = Database::open(&path).unwrap();
    let result = exec(&mut db, "SELECT id, name FROM t");
    match result {
        QueryResult::Rows { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], Value::Integer(2));
            assert_eq!(rows[0][1], Value::Text("bobby".to_string()));
        }
        other => panic!("expected rows, got {other:?}"),
    }

    // A row inserted after reopen must not collide with a row id used before reopen.
    exec(&mut db, "INSERT INTO t VALUES (3, 'carol')");
    let result = exec(&mut db, "SELECT id FROM t ORDER BY id ASC");
    match result {
        QueryResult::Rows { rows, .. } => assert_eq!(rows.len(), 2),
        other => panic!("expected rows, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn malformed_sql_returns_typed_error_without_panic() {
    let path = temp_db_path("malformed");
    let mut db = Database::open(&path).unwrap();

    let cases = [
        "SELEC * FROM t",
        "CREATE TABLE (id INTEGER)",
        "INSERT INTO t VALUES (",
        "SELECT * FROM t WHERE",
        "SELECT * FROM t WHERE a = ",
        "UPDATE t SET",
        "((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((",
        "",
    ];
    for sql in cases {
        let err = run_sql(&mut db, sql);
        assert!(err.is_err(), "expected error for: {sql}");
    }

    let _ = std::fs::remove_file(&path);
}

#[test]
fn deeply_nested_where_expression_does_not_overflow_the_stack() {
    let path = temp_db_path("deep_nest");
    let mut db = Database::open(&path).unwrap();
    exec(&mut db, "CREATE TABLE t (id INTEGER)");

    let opens = "(".repeat(5000);
    let closes = ")".repeat(5000);
    let sql = format!("SELECT * FROM t WHERE {opens}id = 1{closes}");
    let err = run_sql(&mut db, &sql);
    assert!(err.is_err(), "expected a syntax error, not a crash, for pathological nesting");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn duplicate_table_and_missing_table_are_typed_errors() {
    let path = temp_db_path("dup_missing");
    let mut db = Database::open(&path).unwrap();

    exec(&mut db, "CREATE TABLE t (id INTEGER)");
    let err = run_sql(&mut db, "CREATE TABLE t (id INTEGER)").unwrap_err();
    assert!(matches!(err, LsError::Duplicate(_)));

    let err = run_sql(&mut db, "SELECT * FROM nope").unwrap_err();
    assert!(matches!(err, LsError::NotFound(_)));

    let _ = std::fs::remove_file(&path);
}
