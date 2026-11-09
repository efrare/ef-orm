//! TDS adapter of **ef-ORM**.
//!
//! The **only** crate allowed to depend on `tiberius`. It implements the
//! [`ef-orm-driver`] traits over the Tabular Data Stream protocol, including
//! the `Value` ⇄ TDS conversions (type map in
//! `.opencode/skills/ef-orm/references/mssql-dialect.md`).
//!
//! ```text
//! TdsDriver::connect → TdsConn (execute/query/begin) → TdsTx (commit/rollback)
//! ```

mod config;
mod conn;
mod convert;

pub use config::Config;
pub use conn::{TdsConn, TdsDriver, TdsStream, TdsTx};
