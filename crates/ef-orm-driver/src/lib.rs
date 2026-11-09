//! Transport abstraction of **ef-ORM**.
//!
//! Defines the traits the rest of the ORM executes against:
//!
//! - [`Driver`] — opens connections
//! - [`Conn`] — `execute` / `query` / `begin` / `ping`
//! - [`Tx`] — `commit` / `rollback`
//! - [`Row`] / [`RowData`] — typed column access over portable [`Value`]s
//! - [`FromValue`] / [`FromRow`] — cell → Rust type and row → model mapping
//! - [`Pool`] — minimal async pool (semaphore, `SELECT 1`, timeouts)
//!
//! This crate knows **nothing** about TDS or Tiberius; that lives in
//! `ef-orm-driver-tds`. The async surface is RPITIT-based (`async fn` in
//! traits) and `tokio` never appears in a public signature — it only backs
//! the pool's private semaphore/timeout machinery.

mod conn;
mod error;
mod pool;
mod row;

pub use conn::{Conn, Driver, Tx};
pub use error::DriverError;
pub use pool::{Pool, PoolOptions, PooledConn};
pub use row::{FromRow, FromValue, Row, RowData};
