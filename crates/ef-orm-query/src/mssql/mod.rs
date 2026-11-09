//! T-SQL (Microsoft SQL Server) dialect.
//!
//! Rendering rules: `.opencode/skills/ef-orm/references/mssql-dialect.md`.

mod ddl;
mod dml;
mod select;

use ef_orm_core::Value;

use crate::dialect::Dialect;
use crate::error::QueryResult;
use crate::statement::Statement;

/// Renders the query AST as T-SQL for Microsoft SQL Server.
///
/// - identifiers: `[brackets]` (`]` escaped as `]]`)
/// - parameters: `@P1`, `@P2`, … in placeholder order
/// - default schema: `dbo`
#[derive(Debug, Clone)]
pub struct MssqlDialect {
    schema: String,
}

impl MssqlDialect {
    pub fn new() -> Self {
        Self::default()
    }

    /// Overrides the default schema applied to unqualified tables.
    pub fn with_schema(schema: impl Into<String>) -> Self {
        Self {
            schema: schema.into(),
        }
    }
}

impl Default for MssqlDialect {
    fn default() -> Self {
        Self {
            schema: "dbo".to_owned(),
        }
    }
}

impl Dialect for MssqlDialect {
    fn quote_ident(&self, ident: &str) -> String {
        format!("[{}]", ident.replace(']', "]]"))
    }

    fn placeholder(&self, index: usize) -> String {
        format!("@P{index}")
    }

    fn default_schema(&self) -> Option<&str> {
        Some(&self.schema)
    }

    fn build(&self, stmt: &Statement) -> QueryResult<(String, Vec<Value>)> {
        match stmt {
            Statement::Select(s) => Ok(select::render(self, s)),
            Statement::Insert(s) => dml::render_insert(self, s),
            Statement::Update(s) => dml::render_update(self, s),
            Statement::Delete(s) => dml::render_delete(self, s),
            Statement::CreateTable(s) => ddl::render_create_table(self, s),
            Statement::DropTable(s) => ddl::render_drop_table(self, s),
            Statement::AddColumn(s) => ddl::render_add_column(self, s),
            Statement::DropColumn(s) => ddl::render_drop_column(self, s),
            Statement::AlterColumn(s) => ddl::render_alter_column(self, s),
            Statement::CreateIndex(s) => ddl::render_create_index(self, s),
            Statement::DropIndex(s) => ddl::render_drop_index(self, s),
            Statement::AddForeignKey(s) => ddl::render_add_foreign_key(self, s),
            Statement::DropConstraint(s) => ddl::render_drop_constraint(self, s),
        }
    }
}
