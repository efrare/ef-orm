//! The TDS connection: [`TdsDriver`] opens sessions, [`TdsConn`] runs
//! statements over `tiberius::Client`, [`TdsTx`] finishes transactions.

use crate::config::Config;
use crate::convert::{TdsErrorContext, bind_value, cell_to_value, column_data, map_tiberius_error};
use ef_orm_core::Value;
use ef_orm_driver::{Conn, Driver, DriverError, RowData, Tx};
use futures_util::{Stream, StreamExt};
use std::sync::Arc;
use tiberius::{Client, Query, TokenRow};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

/// The stream type behind every [`TdsConn`].
pub type TdsStream = Compat<TcpStream>;

/// A driver that opens SQL Server sessions over TDS (Tiberius).
#[derive(Debug, Clone)]
pub struct TdsDriver {
    config: Config,
}

impl TdsDriver {
    /// Creates a driver from its connection settings.
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    /// The settings this driver dials with.
    pub fn config(&self) -> &Config {
        &self.config
    }
}

impl Driver for TdsDriver {
    type Conn = TdsConn;

    async fn connect(&self) -> Result<TdsConn, DriverError> {
        let addr = self.config.addr();
        let tcp = TcpStream::connect(&addr)
            .await
            .map_err(|error| map_tiberius_error(&error.into(), TdsErrorContext::ConnectTcp))?;
        tcp.set_nodelay(true)
            .map_err(|error| map_tiberius_error(&error.into(), TdsErrorContext::ConfigureTcp))?;

        let client = Client::connect(self.config.to_tiberius(), tcp.compat_write())
            .await
            .map_err(|error| map_tiberius_error(&error, TdsErrorContext::InitializeClient))?;

        Ok(TdsConn { client })
    }
}

/// An open TDS session.
pub struct TdsConn {
    client: Client<TdsStream>,
}

impl Conn for TdsConn {
    type Tx<'a>
        = TdsTx<'a>
    where
        Self: 'a;

    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<u64, DriverError> {
        run_execute(&mut self.client, sql, params).await
    }

    async fn query(&mut self, sql: &str, params: &[Value]) -> Result<Vec<RowData>, DriverError> {
        run_query(&mut self.client, sql, params).await
    }

    async fn begin(&mut self) -> Result<TdsTx<'_>, DriverError> {
        self.client
            .begin_transaction()
            .await
            .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;
        Ok(TdsTx { conn: self })
    }

    fn query_stream(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Stream<Item = Result<RowData, DriverError>> + Send + '_ {
        // The generator owns `sql`/`params` and borrows only the client, so
        // the returned stream needs no self-referential struct.
        async_stream::try_stream! {
            let mut query = Query::new(&sql);
            for value in &params {
                bind_value(&mut query, value);
            }
            let stream = query
                .query(&mut self.client)
                .await
                .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;
            let mut rows = stream.into_row_stream();
            while let Some(row) = rows.next().await {
                let row = row
                    .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;
                yield row_to_data(&row)?;
            }
        }
    }

    async fn bulk_insert(
        &mut self,
        table: &str,
        columns: &[&str],
        rows: &[Vec<Value>],
    ) -> Result<u64, DriverError> {
        if columns.is_empty() {
            return Err(DriverError::Protocol("bulk insert without columns".into()));
        }

        let mut request = self
            .client
            .bulk_insert_columns(table, columns)
            .await
            .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;

        for row in rows {
            if row.len() != columns.len() {
                return Err(DriverError::Protocol(format!(
                    "bulk row has {} values for {} columns",
                    row.len(),
                    columns.len()
                )));
            }
            let mut token_row = TokenRow::with_capacity(columns.len());
            for value in row {
                token_row.push(column_data(value)?);
            }
            request
                .send(token_row)
                .await
                .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;
        }

        let result = request
            .finalize()
            .await
            .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;
        Ok(result.total())
    }
}

/// A transaction guard borrowing its [`TdsConn`] until commit/rollback.
pub struct TdsTx<'a> {
    conn: &'a mut TdsConn,
}

impl Tx for TdsTx<'_> {
    async fn execute(&mut self, sql: &str, params: &[Value]) -> Result<u64, DriverError> {
        // The TDS session is already inside a transaction: every statement on
        // the client takes part in it until commit/rollback.
        run_execute(&mut self.conn.client, sql, params).await
    }

    async fn query(&mut self, sql: &str, params: &[Value]) -> Result<Vec<RowData>, DriverError> {
        run_query(&mut self.conn.client, sql, params).await
    }

    async fn commit(self) -> Result<(), DriverError> {
        self.conn
            .client
            .commit_transaction()
            .await
            .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))
    }

    async fn rollback(self) -> Result<(), DriverError> {
        self.conn
            .client
            .rollback_transaction()
            .await
            .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))
    }
}

/// Runs one statement on a client (inside whatever transaction it holds).
async fn run_execute(
    client: &mut Client<TdsStream>,
    sql: &str,
    params: &[Value],
) -> Result<u64, DriverError> {
    let mut query = Query::new(sql);
    for value in params {
        bind_value(&mut query, value);
    }

    let result = query
        .execute(client)
        .await
        .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;

    Ok(result.rows_affected().iter().sum())
}

/// Runs a query on a client and materializes its first result set.
async fn run_query(
    client: &mut Client<TdsStream>,
    sql: &str,
    params: &[Value],
) -> Result<Vec<RowData>, DriverError> {
    let mut query = Query::new(sql);
    for value in params {
        bind_value(&mut query, value);
    }

    let stream = query
        .query(client)
        .await
        .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;
    let results = stream
        .into_results()
        .await
        .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ExecuteQuery))?;

    // Phase 1 materializes the first result set only; batches with
    // several result sets arrive with the streaming phase.
    materialize(results.into_iter().next().unwrap_or_default())
}

/// Converts one tiberius result set into portable [`RowData`] rows,
/// sharing one `Arc` of column names across the whole set.
fn materialize(rows: Vec<tiberius::Row>) -> Result<Vec<RowData>, DriverError> {
    let columns: Arc<[String]> = match rows.first() {
        Some(row) => row
            .columns()
            .iter()
            .map(|column| column.name().to_string())
            .collect(),
        None => Arc::from(Vec::new()),
    };

    rows.iter()
        .map(|row| {
            let mut values = Vec::with_capacity(columns.len());
            for idx in 0..columns.len() {
                let cell = row
                    .get_column_data(idx)
                    .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ReadCell))?;
                values.push(cell_to_value(cell)?);
            }
            Ok(RowData::new(Arc::clone(&columns), values))
        })
        .collect()
}

/// Converts one streamed row (column names are rebuilt per row).
fn row_to_data(row: &tiberius::Row) -> Result<RowData, DriverError> {
    let columns: Arc<[String]> = row
        .columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect();

    let mut values = Vec::with_capacity(columns.len());
    for idx in 0..columns.len() {
        let cell = row
            .get_column_data(idx)
            .map_err(|error| map_tiberius_error(&error, TdsErrorContext::ReadCell))?;
        values.push(cell_to_value(cell)?);
    }
    Ok(RowData::new(columns, values))
}
