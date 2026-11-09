//! Migration engine of **ef-ORM** (phase 5).
//!
//! - [`Schema`] — fluent DDL builder over the `ef-orm-query` AST (never raw
//!   string SQL)
//! - [`Migration`] — a versioned struct with symmetric `up`/`down` hooks
//! - [`Migrator`] — applies migrations in order, recording them in
//!   `__ef_migrations` inside one transaction per migration
//! - [`introspect`] — reads the live schema from `INFORMATION_SCHEMA` / `sys`
//! - [`diff`] — target schema vs live schema → converging DDL
//! - [`scaffold`] — live schema → `#[derive(Entity, Model)]` Rust source

pub mod diff;
pub mod introspect;
pub mod scaffold;

mod engine;
mod history;
mod migration;
mod schema;

pub use engine::{AppliedMigration, MigrationStatus, Migrator, SqlMigration};
pub use history::TABLE as HISTORY_TABLE;
pub use migration::Migration;
pub use schema::Schema;
