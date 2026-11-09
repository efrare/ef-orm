//! Query AST and dialect layer of **ef-ORM**.
//!
//! The AST (`SelectStatement`, `InsertStatement`, `UpdateStatement`,
//! `DeleteStatement`, [`Expr`]) is engine-neutral; the [`Dialect`] trait
//! renders it into SQL text plus bound parameters. [`MssqlDialect`] is the
//! first implementation — T-SQL rules live in
//! `.opencode/skills/ef-orm/references/mssql-dialect.md`.
//!
//! This is the **only** crate allowed to produce SQL strings.
//!
//! ```text
//! SelectStatement::new("users").column("id").and_where(Expr::col("age").ge(18))
//!   → MssqlDialect::build
//!   → ("SELECT [id] FROM [dbo].[users] WHERE [age] >= @P1", [Int(18)])
//! ```

mod column;
mod ddl;
mod delete;
mod dialect;
mod error;
mod expr;
mod ident;
mod insert;
pub mod mssql;
mod renderer;
mod select;
mod statement;
mod update;

pub use column::{ColumnRef, TextColumn};
pub use ddl::{
    AddColumn, AddForeignKey, AlterColumn, ColumnDef, CreateIndex, CreateTable, DefaultValue,
    DropColumn, DropConstraint, DropIndex, DropTable, RefAction, SqlType,
};
pub use delete::DeleteStatement;
pub use dialect::Dialect;
pub use error::{QueryError, QueryResult};
pub use expr::{CompareOp, Expr};
pub use ident::{Ident, Table};
pub use insert::InsertStatement;
pub use mssql::MssqlDialect;
pub use select::{Order, OrderDir, Page, SelectStatement};
pub use statement::Statement;
pub use update::UpdateStatement;

/// Re-export of the portable value type bound into statements.
pub use ef_orm_core::Value;
