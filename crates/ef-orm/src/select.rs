//! Fluent `SELECT` builder over one entity's columns.

use std::marker::PhantomData;
use std::pin::Pin;

use ef_orm_core::{Column, Model, OrmError};
use ef_orm_driver::{Conn, Driver, FromRow};
use ef_orm_query::{Expr, Ident, OrderDir, Page, SelectStatement, Statement};
use futures_util::{Stream, StreamExt};

use crate::context::DbContext;
use crate::error::map_driver_error;
use crate::statements;

/// A `SELECT` over `T`'s table, built fluently and executed on [`all`](Self::all)
/// / [`first`](Self::first).
///
/// Rows read this way are **tracked** by default (their values become the
/// optimistic-concurrency snapshot of later updates). Use
/// [`as_no_tracking`](Self::as_no_tracking) for read-only queries.
///
/// ```text
/// ctx.dbset::<User>().select()
///     .filter(|c| c.age.gt(18))
///     .order_by(User::columns().age, OrderDir::Desc)
///     .page(1, 20)
///     .all().await?
/// ```
pub struct Select<'a, D: Driver, T: Model> {
    ctx: &'a DbContext<D>,
    stmt: SelectStatement,
    tracking: bool,
    _model: PhantomData<T>,
}

impl<'a, D: Driver, T: Model + FromRow> Select<'a, D, T> {
    pub(crate) fn new(ctx: &'a DbContext<D>) -> Self {
        Self {
            ctx,
            stmt: statements::select_of::<T>(),
            tracking: true,
            _model: PhantomData,
        }
    }

    /// Stops registering the rows this query returns (EF's `AsNoTracking`).
    ///
    /// Untracked rows still load; a later `update`/`delete` for them cannot
    /// diff or guard against originals and falls back to a key-only statement.
    pub fn as_no_tracking(mut self) -> Self {
        self.tracking = false;
        self
    }

    /// Adds a `WHERE` predicate over the typed column references; consecutive
    /// calls are joined with `AND`.
    ///
    /// ```text
    /// .filter(|c| c.age.gt(18).and(c.name.like("A%")))
    /// ```
    pub fn filter<F>(mut self, predicate: F) -> Self
    where
        F: FnOnce(&T::Columns) -> Expr,
    {
        let columns = T::columns();
        self.stmt = self.stmt.and_where(predicate(&columns));
        self
    }

    /// Orders by a typed column (no stringly-typed names).
    pub fn order_by<C: Column>(mut self, column: C, dir: OrderDir) -> Self {
        self.stmt = self.stmt.order_by(column.name(), dir);
        self
    }

    /// Paginates (1-based page number, page size → `OFFSET … FETCH`).
    pub fn page(mut self, number: u64, size: u64) -> Self {
        self.stmt = self.stmt.page(Page::new(number, size));
        self
    }

    /// Restricts the projection to `names` (default: every entity column).
    ///
    /// Useful with [`all_as`](Self::all_as)/[`first_as`](Self::first_as) to
    /// read a **partial row** into a custom `FromRow` type.
    pub fn columns<I, N>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = N>,
        N: Into<Ident>,
    {
        self.stmt.columns = names.into_iter().map(Into::into).collect();
        self
    }

    /// Runs the query and maps every row to a partial `P` (columns by name).
    ///
    /// Partial rows are **not tracked** — they lack the full-column snapshot
    /// the change tracker needs.
    pub async fn all_as<P: FromRow>(self) -> Result<Vec<P>, OrmError> {
        self.fetch_as().await
    }

    /// Runs the query limited to one row and maps it to a partial `P`.
    pub async fn first_as<P: FromRow>(mut self) -> Result<Option<P>, OrmError> {
        self.stmt = self.stmt.limit(1);
        self.fetch_as().await.map(|mut rows| rows.pop())
    }

    /// Streams the query rows over a caller-owned connection.
    ///
    /// Streaming does **not** register originals in the change tracker (the
    /// rows are consumed one at a time). Get a connection with
    /// [`DbContext::checkout`](crate::DbContext::checkout):
    ///
    /// ```text
    /// let mut conn = ctx.checkout().await?;
    /// let mut rows = ctx.dbset::<User>().select().stream(&mut *conn).await?;
    /// while let Some(user) = rows.next().await { let user = user?; … }
    /// ```
    pub async fn stream<'c>(
        self,
        conn: &'c mut D::Conn,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<T, OrmError>> + Send + 'c>>, OrmError> {
        let (sql, params) = self.ctx.build(&Statement::Select(self.stmt))?;
        let stream = conn.query_stream(sql, params).map(|item| {
            item.map_err(map_driver_error)
                .and_then(|row| T::from_row(&row))
        });
        Ok(Box::pin(stream))
    }

    /// Runs the query and maps every row to `T`.
    pub async fn all(self) -> Result<Vec<T>, OrmError> {
        self.fetch().await
    }

    /// Runs the query limited to one row (`TOP (1)`), if any.
    pub async fn first(mut self) -> Result<Option<T>, OrmError> {
        self.stmt = self.stmt.limit(1);
        self.fetch().await.map(|mut rows| rows.pop())
    }

    async fn fetch_as<P: FromRow>(self) -> Result<Vec<P>, OrmError> {
        let (sql, params) = self.ctx.build(&Statement::Select(self.stmt))?;
        let mut conn = self.ctx.pool().checkout().await.map_err(map_driver_error)?;
        let rows = conn.query(&sql, &params).await.map_err(map_driver_error)?;
        rows.iter().map(P::from_row).collect()
    }

    async fn fetch(self) -> Result<Vec<T>, OrmError> {
        let (sql, params) = self.ctx.build(&Statement::Select(self.stmt))?;
        let mut conn = self.ctx.pool().checkout().await.map_err(map_driver_error)?;
        let rows = conn.query(&sql, &params).await.map_err(map_driver_error)?;

        let mut models = Vec::with_capacity(rows.len());
        for row in &rows {
            models.push(T::from_row(row)?);
            if self.tracking {
                self.ctx.track::<T>(row)?;
            }
        }
        Ok(models)
    }
}
