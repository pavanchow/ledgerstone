//! Executes a parsed `Statement` against a `Database`. Type-checks every
//! value against its column's declared type before it touches storage, so a
//! bad INSERT or UPDATE fails cleanly instead of corrupting a row.

use crate::error::{LsError, LsResult};
use crate::parser::{ColumnDef, CompareOp, Expr, OrderBy, OrderDir, SelectColumns, Statement};
use crate::storage::{Database, StoredColumn, Table};
use crate::types::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum QueryResult {
    Ok,
    RowsAffected(usize),
    Rows {
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    },
}

pub fn execute(db: &mut Database, stmt: Statement) -> LsResult<QueryResult> {
    match stmt {
        Statement::CreateTable { table, columns } => {
            let stored: Vec<StoredColumn> = columns
                .into_iter()
                .map(|ColumnDef { name, ty }| StoredColumn { name, ty })
                .collect();
            db.create_table(&table, stored)?;
            Ok(QueryResult::Ok)
        }
        Statement::Insert {
            table,
            columns,
            values,
        } => exec_insert(db, &table, columns, values),
        Statement::Select {
            table,
            columns,
            where_clause,
            order_by,
            limit,
        } => exec_select(db, &table, columns, where_clause, order_by, limit),
        Statement::Delete { table, where_clause } => exec_delete(db, &table, where_clause),
        Statement::Update {
            table,
            assignments,
            where_clause,
        } => exec_update(db, &table, assignments, where_clause),
    }
}

fn check_type(col_ty: crate::types::ColumnType, val: &Value) -> LsResult<()> {
    if val.type_of() != col_ty {
        return Err(LsError::Type(format!(
            "expected {col_ty}, got {}",
            val.type_of()
        )));
    }
    Ok(())
}

fn exec_insert(
    db: &mut Database,
    table: &str,
    columns: Option<Vec<String>>,
    values: Vec<Value>,
) -> LsResult<QueryResult> {
    let ordered_values = {
        let t = db.table(table)?;
        match columns {
            None => {
                if values.len() != t.columns.len() {
                    return Err(LsError::Type(format!(
                        "table '{table}' has {} columns, got {} values",
                        t.columns.len(),
                        values.len()
                    )));
                }
                for (col, val) in t.columns.iter().zip(values.iter()) {
                    check_type(col.ty, val)?;
                }
                values
            }
            Some(names) => {
                if names.len() != values.len() {
                    return Err(LsError::Type(format!(
                        "{} columns named but {} values given",
                        names.len(),
                        values.len()
                    )));
                }
                let mut ordered: Vec<Option<Value>> = vec![None; t.columns.len()];
                for (name, val) in names.iter().zip(values.into_iter()) {
                    let idx = t
                        .column_index(name)
                        .ok_or_else(|| LsError::NotFound(format!("column '{name}' does not exist")))?;
                    check_type(t.columns[idx].ty, &val)?;
                    ordered[idx] = Some(val);
                }
                let mut out = Vec::with_capacity(ordered.len());
                for (i, slot) in ordered.into_iter().enumerate() {
                    match slot {
                        Some(v) => out.push(v),
                        None => {
                            return Err(LsError::Type(format!(
                                "missing value for column '{}'",
                                t.columns[i].name
                            )))
                        }
                    }
                }
                out
            }
        }
    };
    db.insert_row(table, ordered_values)?;
    Ok(QueryResult::RowsAffected(1))
}

fn matching_row_ids(t: &Table, where_clause: &Option<Expr>) -> LsResult<Vec<u64>> {
    let mut ids: Vec<u64> = Vec::new();
    for (row_id, row) in t.rows.iter() {
        let keep = match where_clause {
            None => true,
            Some(expr) => eval_expr(t, row, expr)?,
        };
        if keep {
            ids.push(*row_id);
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

fn eval_expr(t: &Table, row: &[Value], expr: &Expr) -> LsResult<bool> {
    match expr {
        Expr::Compare { column, op, value } => {
            let idx = t
                .column_index(column)
                .ok_or_else(|| LsError::NotFound(format!("column '{column}' does not exist")))?;
            let cell = row
                .get(idx)
                .ok_or_else(|| LsError::Storage("row shorter than schema".into()))?;
            Ok(compare(cell, op, value))
        }
        Expr::And(l, r) => Ok(eval_expr(t, row, l)? && eval_expr(t, row, r)?),
        Expr::Or(l, r) => Ok(eval_expr(t, row, l)? || eval_expr(t, row, r)?),
    }
}

fn compare(cell: &Value, op: &CompareOp, literal: &Value) -> bool {
    match op {
        CompareOp::Eq => cell.partial_compare(literal) == Some(std::cmp::Ordering::Equal),
        CompareOp::NotEq => cell.partial_compare(literal) != Some(std::cmp::Ordering::Equal),
        CompareOp::Lt => cell.partial_compare(literal) == Some(std::cmp::Ordering::Less),
        CompareOp::Gt => cell.partial_compare(literal) == Some(std::cmp::Ordering::Greater),
        CompareOp::Le => matches!(
            cell.partial_compare(literal),
            Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal)
        ),
        CompareOp::Ge => matches!(
            cell.partial_compare(literal),
            Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal)
        ),
    }
}

fn exec_select(
    db: &Database,
    table: &str,
    columns: SelectColumns,
    where_clause: Option<Expr>,
    order_by: Option<OrderBy>,
    limit: Option<u64>,
) -> LsResult<QueryResult> {
    let t = db.table(table)?;
    let mut ids = matching_row_ids(t, &where_clause)?;

    let out_cols: Vec<String> = match &columns {
        SelectColumns::All => t.columns.iter().map(|c| c.name.clone()).collect(),
        SelectColumns::Named(names) => {
            for n in names {
                if t.column_index(n).is_none() {
                    return Err(LsError::NotFound(format!("column '{n}' does not exist")));
                }
            }
            names.clone()
        }
    };
    let out_idx: LsResult<Vec<usize>> = out_cols
        .iter()
        .map(|n| {
            t.column_index(n)
                .ok_or_else(|| LsError::NotFound(format!("column '{n}' does not exist")))
        })
        .collect();
    let out_idx = out_idx?;

    if let Some(ob) = &order_by {
        let sort_idx = t
            .column_index(&ob.column)
            .ok_or_else(|| LsError::NotFound(format!("column '{}' does not exist", ob.column)))?;
        ids.sort_by(|a, b| {
            let ra = &t.rows[a];
            let rb = &t.rows[b];
            let ord = ra[sort_idx]
                .partial_compare(&rb[sort_idx])
                .unwrap_or(std::cmp::Ordering::Equal);
            match ob.dir {
                OrderDir::Asc => ord,
                OrderDir::Desc => ord.reverse(),
            }
        });
    }

    if let Some(n) = limit {
        ids.truncate(n as usize);
    }

    let rows: Vec<Vec<Value>> = ids
        .iter()
        .map(|id| {
            let row = &t.rows[id];
            out_idx.iter().map(|&i| row[i].clone()).collect()
        })
        .collect();

    Ok(QueryResult::Rows {
        columns: out_cols,
        rows,
    })
}

fn exec_delete(db: &mut Database, table: &str, where_clause: Option<Expr>) -> LsResult<QueryResult> {
    let ids = {
        let t = db.table(table)?;
        matching_row_ids(t, &where_clause)?
    };
    for id in &ids {
        db.delete_row(table, *id)?;
    }
    Ok(QueryResult::RowsAffected(ids.len()))
}

fn exec_update(
    db: &mut Database,
    table: &str,
    assignments: Vec<(String, Value)>,
    where_clause: Option<Expr>,
) -> LsResult<QueryResult> {
    let (ids, set_idx) = {
        let t = db.table(table)?;
        let mut set_idx = Vec::with_capacity(assignments.len());
        for (name, val) in &assignments {
            let idx = t
                .column_index(name)
                .ok_or_else(|| LsError::NotFound(format!("column '{name}' does not exist")))?;
            check_type(t.columns[idx].ty, val)?;
            set_idx.push(idx);
        }
        (matching_row_ids(t, &where_clause)?, set_idx)
    };

    for id in &ids {
        let mut row = db.table(table)?.rows[id].clone();
        for (idx, (_, val)) in set_idx.iter().zip(assignments.iter()) {
            row[*idx] = val.clone();
        }
        db.update_row(table, *id, row)?;
    }
    Ok(QueryResult::RowsAffected(ids.len()))
}
