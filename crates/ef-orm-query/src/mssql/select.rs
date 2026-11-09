//! T-SQL rendering of `SELECT` (incl. `TOP` and `OFFSET…FETCH`).

use ef_orm_core::Value;

use crate::mssql::MssqlDialect;
use crate::renderer::Renderer;
use crate::select::SelectStatement;

pub(crate) fn render(dialect: &MssqlDialect, stmt: &SelectStatement) -> (String, Vec<Value>) {
    let mut r = Renderer::new(dialect);

    r.push("SELECT ");
    // `limit` alone ⇒ TOP; with `offset` the pagination clause takes over.
    if let Some(limit) = stmt.limit.filter(|_| stmt.offset.is_none()) {
        r.push("TOP (");
        r.param(Value::Int(i64::try_from(limit).unwrap_or(i64::MAX)));
        r.push(") ");
    }

    if stmt.columns.is_empty() {
        r.push("*");
    } else {
        for (i, column) in stmt.columns.iter().enumerate() {
            if i > 0 {
                r.push(", ");
            }
            r.ident(column);
        }
    }

    r.push(" FROM ");
    r.table(&stmt.table);

    if let Some(filter) = &stmt.filter {
        r.push(" WHERE ");
        r.expr(filter);
    }

    let has_order = !stmt.order_by.is_empty();
    if has_order {
        r.push(" ORDER BY ");
        for (i, order) in stmt.order_by.iter().enumerate() {
            if i > 0 {
                r.push(", ");
            }
            r.expr(&order.expr);
            r.push(" ");
            r.push(order.dir.as_str());
        }
    }

    if let Some(offset) = stmt.offset {
        // `OFFSET … ROWS` requires an ORDER BY in T-SQL.
        if !has_order {
            r.push(" ORDER BY (SELECT NULL)");
        }
        r.push(" OFFSET ");
        r.param(Value::Int(i64::try_from(offset).unwrap_or(i64::MAX)));
        r.push(" ROWS");
        if let Some(limit) = stmt.limit {
            r.push(" FETCH NEXT ");
            r.param(Value::Int(i64::try_from(limit).unwrap_or(i64::MAX)));
            r.push(" ROWS ONLY");
        }
    }

    r.finish()
}
