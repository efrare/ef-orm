//! **ef-ORM** — an async, MSSQL-first ORM for Rust.
//!
//! Public facade combining the workspace layers into the API users touch:
//!
//! - [`DbContext`] — session of work over a connection pool
//! - [`DbSet<T>`] — typed entity access: `insert`, `find`, `update`, `delete`,
//!   fluent `select()`
//! - [`Select<T>`] — fluent query builder over typed column references
//! - [`Entity`], [`Model`], [`Column`], [`PrimaryKey`] — metadata traits,
//!   implemented by the re-exported [`Entity`]/[`Model`] derives
//! - [`OrmError`] — the only error type users ever see
//!
//! Layers behind the facade: `ef-orm-core` (domain), `ef-orm-query` (AST +
//! T-SQL dialect), `ef-orm-driver` (transport traits + pool),
//! `ef-orm-driver-tds` (Tiberius/TDS).
//!
//! ```text
//! ctx.dbset::<User>().select().filter(|c| c.age.gt(18)).page(1, 20).all()
//!   → AST → MssqlDialect → @P1… → pool → TDS → rows → User models
//! ```

// Lets derive-generated code inside this crate spell `::ef_orm::…` paths
// (same trick as serde: `extern crate self as serde`).
extern crate self as ef_orm;

mod change_tracker;
mod concurrency;
mod context;
mod error;
mod raw_sql;
mod save;
mod select;
mod set;
mod staged;
mod statements;

#[cfg(test)]
mod derive_tests;
#[cfg(test)]
mod set_tests;
#[cfg(test)]
mod test_support;

pub use change_tracker::EntityState;
pub use context::DbContext;
pub use ef_orm_core::{Column, Entity, Model, OrmError, PrimaryKey, Result, Value};
pub use ef_orm_derive::{Entity, Model};
pub use ef_orm_driver::{
    Conn, Driver, DriverError, FromRow, FromValue, Pool, PoolOptions, PooledConn, Row, RowData, Tx,
};
pub use ef_orm_query::{
    ColumnRef, CompareOp, Expr, MssqlDialect, OrderDir, Page, SelectStatement, TextColumn,
};
pub use raw_sql::RawSql;
pub use save::SaveChangesReport;
pub use select::Select;
pub use set::DbSet;
pub use staged::StagedInsert;
