//! Fluent DDL builder: a migration accumulates `ef-orm-query` statements.
//!
//! ```text
//! fn up(schema: &mut Schema) {
//!     schema.create_table(
//!         CreateTable::new("users")
//!             .column(ColumnDef::new("id", SqlType::Int).identity().primary_key())
//!             .column(ColumnDef::new("name", SqlType::NVarChar(100)).not_null()),
//!     );
//!     schema.create_index(CreateIndex::new("ix_users_name", "users").column("name"));
//! }
//! ```

use ef_orm_query::{
    AddColumn, AddForeignKey, AlterColumn, ColumnDef, CreateIndex, CreateTable, DropColumn,
    DropConstraint, DropIndex, DropTable, Ident, Statement, Table,
};

/// Ordered DDL statements for one migration direction.
#[derive(Debug, Default)]
pub struct Schema {
    statements: Vec<Statement>,
}

impl Schema {
    /// An empty schema (no statements).
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends any statement (DDL or, exceptionally, DML).
    pub fn push(&mut self, statement: impl Into<Statement>) {
        self.statements.push(statement.into());
    }

    /// `CREATE TABLE …` (see [`CreateTable`]).
    pub fn create_table(&mut self, table: CreateTable) {
        self.push(table);
    }

    /// `DROP TABLE …`.
    pub fn drop_table(&mut self, table: impl Into<Table>) {
        self.push(DropTable::new(table));
    }

    /// `ALTER TABLE … ADD <column>`.
    pub fn add_column(&mut self, table: impl Into<Table>, column: ColumnDef) {
        self.push(AddColumn::new(table, column));
    }

    /// `ALTER TABLE … DROP COLUMN <column>`.
    pub fn drop_column(&mut self, table: impl Into<Table>, column: impl Into<Ident>) {
        self.push(DropColumn::new(table, column));
    }

    /// `ALTER TABLE … ALTER COLUMN <column>`.
    pub fn alter_column(&mut self, table: impl Into<Table>, column: ColumnDef) {
        self.push(AlterColumn::new(table, column));
    }

    /// `CREATE [UNIQUE] INDEX …` (see [`CreateIndex`]).
    pub fn create_index(&mut self, index: CreateIndex) {
        self.push(index);
    }

    /// `DROP INDEX … ON <table>`.
    pub fn drop_index(&mut self, name: impl Into<Ident>, table: impl Into<Table>) {
        self.push(DropIndex::new(name, table));
    }

    /// `ALTER TABLE … ADD CONSTRAINT … FOREIGN KEY …` (see [`AddForeignKey`]).
    pub fn add_foreign_key(&mut self, foreign_key: AddForeignKey) {
        self.push(foreign_key);
    }

    /// `ALTER TABLE … DROP CONSTRAINT …`.
    pub fn drop_constraint(&mut self, table: impl Into<Table>, name: impl Into<Ident>) {
        self.push(DropConstraint::new(table, name));
    }

    /// The accumulated statements, in call order.
    pub fn as_statements(&self) -> &[Statement] {
        &self.statements
    }

    /// Consumes the builder, returning the statements.
    pub fn statements(self) -> Vec<Statement> {
        self.statements
    }

    /// Number of statements queued.
    pub fn len(&self) -> usize {
        self.statements.len()
    }

    /// True when no statement was queued.
    pub fn is_empty(&self) -> bool {
        self.statements.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ef_orm_query::{Dialect, MssqlDialect, SqlType};

    fn render(stmt: &Statement) -> String {
        MssqlDialect::new().build(stmt).expect("renders").0
    }

    #[test]
    fn accumulates_statements_in_call_order() {
        let mut schema = Schema::new();
        schema.create_table(
            CreateTable::new("users")
                .column(ColumnDef::new("id", SqlType::Int).identity().primary_key())
                .column(ColumnDef::new("name", SqlType::NVarChar(50)).not_null()),
        );
        schema.drop_table("old_users");
        schema.add_column("users", ColumnDef::new("age", SqlType::Int).not_null());
        schema.create_index(CreateIndex::new("ix_users_name", "users").column("name"));

        assert_eq!(schema.len(), 4);
        let sql: Vec<String> = schema.as_statements().iter().map(render).collect();
        assert_eq!(
            sql,
            vec![
                "CREATE TABLE [dbo].[users] ([id] INT IDENTITY(1,1) NOT NULL PRIMARY KEY, \
                 [name] NVARCHAR(50) NOT NULL)"
                    .to_string(),
                "DROP TABLE [dbo].[old_users]".to_string(),
                "ALTER TABLE [dbo].[users] ADD [age] INT NOT NULL".to_string(),
                "CREATE INDEX [ix_users_name] ON [dbo].[users] ([name])".to_string(),
            ]
        );
    }

    #[test]
    fn empty_schema_reports_itself_empty() {
        let schema = Schema::new();
        assert!(schema.is_empty());
        assert_eq!(schema.len(), 0);
        assert!(schema.statements().is_empty());
    }
}
