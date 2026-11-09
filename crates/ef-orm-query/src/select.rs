//! `SELECT` statement AST.

use crate::expr::{Expr, combine_filter};
use crate::ident::{Ident, Table};

/// Sort direction of one `ORDER BY` item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDir {
    Asc,
    Desc,
}

impl OrderDir {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// One `ORDER BY` item.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub expr: Expr,
    pub dir: OrderDir,
}

/// A page request (1-based), translated to offset/limit semantics.
///
/// `page(2, 10)` ⇒ offset 10, limit 10 (T-SQL: `OFFSET … FETCH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub number: u64,
    pub size: u64,
}

impl Page {
    pub fn new(number: u64, size: u64) -> Self {
        Self {
            number: number.max(1),
            size,
        }
    }

    /// Rows to skip (never negative).
    pub fn offset(&self) -> u64 {
        self.number.saturating_sub(1).saturating_mul(self.size)
    }
}

/// A `SELECT` statement.
///
/// Rendering rules (T-SQL, see `references/mssql-dialect.md`):
/// - no columns ⇒ `*`
/// - `limit` without `offset` ⇒ `TOP (@P1)`
/// - `offset` present ⇒ `OFFSET … ROWS [FETCH NEXT … ROWS ONLY]`, which
///   requires `ORDER BY` — the dialect injects `ORDER BY (SELECT NULL)` when
///   the user did not order.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectStatement {
    pub columns: Vec<Ident>,
    pub table: Table,
    pub filter: Option<Expr>,
    pub order_by: Vec<Order>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

impl SelectStatement {
    pub fn new(table: impl Into<Table>) -> Self {
        Self {
            columns: Vec::new(),
            table: table.into(),
            filter: None,
            order_by: Vec::new(),
            limit: None,
            offset: None,
        }
    }

    pub fn column(mut self, name: impl Into<Ident>) -> Self {
        self.columns.push(name.into());
        self
    }

    pub fn columns<I>(mut self, names: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Ident>,
    {
        self.columns.extend(names.into_iter().map(Into::into));
        self
    }

    /// Adds a `WHERE` predicate; consecutive calls are joined with `AND`.
    pub fn and_where(mut self, filter: impl Into<Expr>) -> Self {
        combine_filter(&mut self.filter, filter.into());
        self
    }

    /// Orders by a column.
    pub fn order_by(mut self, name: impl Into<Ident>, dir: OrderDir) -> Self {
        self.order_by.push(Order {
            expr: Expr::Column(name.into()),
            dir,
        });
        self
    }

    /// Orders by an arbitrary expression (functions, arithmetic…).
    pub fn order_by_expr(mut self, expr: impl Into<Expr>, dir: OrderDir) -> Self {
        self.order_by.push(Order {
            expr: expr.into(),
            dir,
        });
        self
    }

    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Sets both `limit` and `offset` from a [`Page`] (1-based).
    pub fn page(mut self, page: Page) -> Self {
        self.limit = Some(page.size);
        self.offset = Some(page.offset());
        self
    }
}
