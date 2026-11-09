//! The `__ef_migrations` history table: DDL plus the statements the engine uses.

use ef_orm_query::{
    ColumnDef, CreateTable, DefaultValue, DeleteStatement, Expr, InsertStatement, OrderDir,
    SelectStatement, SqlType, Table,
};

/// Name of the history table (kept unqualified; `dbo` applies by default).
pub const TABLE: &str = "__ef_migrations";

/// `CREATE TABLE __ef_migrations (version, name, applied_at)`.
pub fn table() -> CreateTable {
    CreateTable::new(TABLE)
        .column(
            ColumnDef::new("version", SqlType::NVarChar(64))
                .not_null()
                .primary_key(),
        )
        .column(ColumnDef::new("name", SqlType::NVarChar(200)).not_null())
        .column(
            ColumnDef::new("applied_at", SqlType::DateTime2)
                .not_null()
                .default(DefaultValue::SysUtcDateTime),
        )
}

/// `SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE …` (exists probe).
pub fn select_exists() -> SelectStatement {
    SelectStatement::new(Table::with_schema("INFORMATION_SCHEMA", "TABLES"))
        .columns(["TABLE_NAME"])
        .and_where(Expr::col("TABLE_SCHEMA").eq(Expr::val("dbo")))
        .and_where(Expr::col("TABLE_NAME").eq(Expr::val(TABLE)))
}

/// `SELECT version, name FROM __ef_migrations ORDER BY version`.
pub fn select_applied() -> SelectStatement {
    SelectStatement::new(TABLE)
        .columns(["version", "name"])
        .order_by("version", OrderDir::Asc)
}

/// `INSERT INTO __ef_migrations (version, name) VALUES (@P1, @P2)`.
pub fn insert_applied(version: &str, name: &str) -> InsertStatement {
    InsertStatement::new(TABLE)
        .columns(["version", "name"])
        .values([version, name])
}

/// `DELETE FROM __ef_migrations WHERE version = @P1`.
pub fn delete_applied(version: &str) -> DeleteStatement {
    DeleteStatement::new(TABLE).and_where(Expr::col("version").eq(Expr::val(version)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ef_orm_query::{Dialect, MssqlDialect, Statement};

    fn render(stmt: impl Into<Statement>) -> String {
        MssqlDialect::new().build(&stmt.into()).expect("renders").0
    }

    #[test]
    fn history_table_keeps_a_unique_version_and_a_utc_default() {
        assert_eq!(
            render(table()),
            "CREATE TABLE [dbo].[__ef_migrations] (\
             [version] NVARCHAR(64) NOT NULL PRIMARY KEY, \
             [name] NVARCHAR(200) NOT NULL, \
             [applied_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME())"
        );
    }

    #[test]
    fn engine_statements_use_the_history_table() {
        assert_eq!(
            render(select_exists()),
            "SELECT [TABLE_NAME] FROM [INFORMATION_SCHEMA].[TABLES] \
             WHERE [TABLE_SCHEMA] = @P1 AND [TABLE_NAME] = @P2"
        );
        assert_eq!(
            render(select_applied()),
            "SELECT [version], [name] FROM [dbo].[__ef_migrations] ORDER BY [version] ASC"
        );
        assert_eq!(
            render(insert_applied("v1", "create_users")),
            "INSERT INTO [dbo].[__ef_migrations] ([version], [name]) VALUES (@P1, @P2)"
        );
        assert_eq!(
            render(delete_applied("v1")),
            "DELETE FROM [dbo].[__ef_migrations] WHERE [version] = @P1"
        );
    }
}
