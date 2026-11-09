//! T-SQL rendering of `INSERT` / `UPDATE` / `DELETE` (incl. `OUTPUT`).

use ef_orm_core::Value;

use crate::delete::DeleteStatement;
use crate::error::{QueryError, QueryResult};
use crate::ident::Ident;
use crate::insert::InsertStatement;
use crate::mssql::MssqlDialect;
use crate::renderer::Renderer;
use crate::update::UpdateStatement;

pub(crate) fn render_insert(
    dialect: &MssqlDialect,
    stmt: &InsertStatement,
) -> QueryResult<(String, Vec<Value>)> {
    if stmt.columns.is_empty() {
        return Err(QueryError::invalid("INSERT without columns"));
    }
    if stmt.rows.is_empty() {
        return Err(QueryError::invalid("INSERT without values"));
    }
    for (i, row) in stmt.rows.iter().enumerate() {
        if row.len() != stmt.columns.len() {
            return Err(QueryError::invalid(format!(
                "INSERT row {i} has {} values for {} columns",
                row.len(),
                stmt.columns.len()
            )));
        }
    }

    let mut r = Renderer::new(dialect);
    r.push("INSERT INTO ");
    r.table(&stmt.table);
    r.push(" (");
    push_ident_list(&mut r, &stmt.columns);
    r.push(")");
    push_output(&mut r, &stmt.output, "INSERTED");
    r.push(" VALUES ");
    for (i, row) in stmt.rows.iter().enumerate() {
        if i > 0 {
            r.push(", ");
        }
        r.push("(");
        for (j, value) in row.iter().enumerate() {
            if j > 0 {
                r.push(", ");
            }
            r.param(value.clone());
        }
        r.push(")");
    }
    Ok(r.finish())
}

pub(crate) fn render_update(
    dialect: &MssqlDialect,
    stmt: &UpdateStatement,
) -> QueryResult<(String, Vec<Value>)> {
    if stmt.assignments.is_empty() {
        return Err(QueryError::invalid("UPDATE without SET assignments"));
    }

    let mut r = Renderer::new(dialect);
    r.push("UPDATE ");
    r.table(&stmt.table);
    r.push(" SET ");
    for (i, (column, value)) in stmt.assignments.iter().enumerate() {
        if i > 0 {
            r.push(", ");
        }
        r.ident(column);
        r.push(" = ");
        r.param(value.clone());
    }
    push_output(&mut r, &stmt.output, "INSERTED");
    if let Some(filter) = &stmt.filter {
        r.push(" WHERE ");
        r.expr(filter);
    }
    Ok(r.finish())
}

pub(crate) fn render_delete(
    dialect: &MssqlDialect,
    stmt: &DeleteStatement,
) -> QueryResult<(String, Vec<Value>)> {
    let mut r = Renderer::new(dialect);
    r.push("DELETE FROM ");
    r.table(&stmt.table);
    push_output(&mut r, &stmt.output, "DELETED");
    if let Some(filter) = &stmt.filter {
        r.push(" WHERE ");
        r.expr(filter);
    }
    Ok(r.finish())
}

fn push_ident_list(r: &mut Renderer<'_>, idents: &[Ident]) {
    for (i, ident) in idents.iter().enumerate() {
        if i > 0 {
            r.push(", ");
        }
        r.ident(ident);
    }
}

/// ` OUTPUT INSERTED.[col]` / ` OUTPUT DELETED.[col]` (empty when none).
fn push_output(r: &mut Renderer<'_>, output: &[Ident], source: &str) {
    if output.is_empty() {
        return;
    }
    r.push(" OUTPUT ");
    for (i, ident) in output.iter().enumerate() {
        if i > 0 {
            r.push(", ");
        }
        r.push(source);
        r.push(".");
        r.ident(ident);
    }
}
