//! Transport traits: [`Driver`] opens connections, [`Conn`] runs statements,
//! [`Tx`] finishes transactions.
//!
//! The surface is RPITIT-based but **desugared** to
//! `fn … -> impl Future<Output = …> + Send` instead of `async fn`: that keeps
//! the zero-cost, non-boxed surface while making every future `Send`, which
//! pools and spawned tasks require (and which `async fn` in traits cannot
//! guarantee — see the `async_fn_in_trait` lint).

use crate::{DriverError, Row, RowData};
use ef_orm_core::Value;
use futures_core::Stream;
use std::future::Future;

/// Opens connections to a database.
pub trait Driver: Send + Sync {
    /// The connection type produced by this driver.
    type Conn: Conn;

    /// Opens a new connection with the driver's configuration.
    fn connect(&self) -> impl Future<Output = Result<Self::Conn, DriverError>> + Send;
}

/// An open connection: execute, query, transactions.
pub trait Conn: Send {
    /// Transaction guard returned by [`Conn::begin`] (borrowing the
    /// connection until commit/rollback consumes it).
    type Tx<'a>: Tx
    where
        Self: 'a;

    /// Executes a statement, returning the total number of affected rows.
    fn execute(
        &mut self,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Result<u64, DriverError>> + Send;

    /// Runs a query and materializes its **first result set** into portable
    /// [`RowData`] rows (later result sets of a batch are ignored).
    fn query(
        &mut self,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Result<Vec<RowData>, DriverError>> + Send;

    /// Starts a transaction scoped to this connection.
    ///
    /// The guard must be consumed by [`Tx::commit`] or [`Tx::rollback`];
    /// dropping it without either leaves the connection inside an open
    /// transaction (the facade's `UnitOfWork` guarantees the pairing).
    fn begin(&mut self) -> impl Future<Output = Result<Self::Tx<'_>, DriverError>> + Send;

    /// Bulk-inserts `rows` into `table`, returning the inserted row count.
    ///
    /// `table` and `columns` are interpolated into the statement (SQL Server
    /// cannot parameterize identifiers), so they must be trusted, validated
    /// names. Each row must follow the column list order.
    ///
    /// The default implementation reports "unsupported"; drivers that can do
    /// bulk copy (TDS) override it.
    fn bulk_insert(
        &mut self,
        table: &str,
        columns: &[&str],
        rows: &[Vec<Value>],
    ) -> impl Future<Output = Result<u64, DriverError>> + Send {
        let _ = (table, columns, rows);
        async move {
            Err(DriverError::Protocol(
                "bulk insert is not supported by this driver".into(),
            ))
        }
    }

    /// Streams the rows of a query without materializing the whole result.
    ///
    /// The SQL text and parameters are **owned** by the stream, so it can
    /// outlive the caller's locals; it borrows only the connection. The
    /// default implementation yields nothing (drivers that can stream
    /// override it).
    fn query_stream(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Stream<Item = Result<RowData, DriverError>> + Send + '_ {
        let _ = (sql, params);
        futures_util::stream::empty()
    }

    /// Verifies the connection with `SELECT 1 AS [health_check]`.
    fn ping(&mut self) -> impl Future<Output = Result<(), DriverError>> + Send {
        async move {
            let rows = self.query("SELECT 1 AS [health_check]", &[]).await?;
            let row = rows
                .first()
                .ok_or_else(|| DriverError::Protocol("health check returned no rows".into()))?;
            let value: Option<i32> = row
                .get(0)
                .map_err(|error| DriverError::Protocol(error.to_string()))?;
            match value {
                Some(1) => Ok(()),
                _ => Err(DriverError::Protocol(
                    "health check returned an unexpected value".into(),
                )),
            }
        }
    }
}

/// An active transaction: statements run through [`Tx::execute`] /
/// [`Tx::query`] take part in it, and consuming `commit`/`rollback` releases
/// it. Dropping it without either leaves the connection inside an open
/// transaction (the facade's `UnitOfWork` guarantees the pairing).
pub trait Tx: Send {
    /// Executes a statement inside the transaction, returning affected rows.
    fn execute(
        &mut self,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Result<u64, DriverError>> + Send;

    /// Runs a query inside the transaction and materializes its **first
    /// result set** (same contract as [`Conn::query`]).
    fn query(
        &mut self,
        sql: &str,
        params: &[Value],
    ) -> impl Future<Output = Result<Vec<RowData>, DriverError>> + Send;

    /// Persists the transaction's changes.
    fn commit(self) -> impl Future<Output = Result<(), DriverError>> + Send;

    /// Discards the transaction's changes.
    fn rollback(self) -> impl Future<Output = Result<(), DriverError>> + Send;
}
