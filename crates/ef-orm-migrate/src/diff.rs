//! Schema diff: compare the target schema with the live one and emit the DDL
//! that converges the database.
//!
//! The diff is intentionally table/column based (types + nullability). Extra
//! live columns/tables are **dropped** to converge; review the output before
//! applying it to production.

use std::collections::BTreeSet;

use ef_orm_query::{
    AddColumn, AlterColumn, ColumnDef, CreateTable, DropColumn, DropTable, Statement,
};

use crate::introspect::{LiveColumn, LiveSchema, LiveTable};

/// Statements that make `actual` match `expected`.
///
/// Order: create/alter existing tables in `expected` order, then drop tables
/// that only exist in the database.
pub fn diff(expected: &LiveSchema, actual: &LiveSchema) -> Vec<Statement> {
    let mut statements = Vec::new();

    for table in &expected.tables {
        match actual.table(&table.name) {
            None => statements.push(create_table(table).into()),
            Some(live) => diff_table(table, live, &mut statements),
        }
    }

    let expected_names: BTreeSet<String> = expected
        .tables
        .iter()
        .map(|table| table.name.to_lowercase())
        .collect();
    for table in &actual.tables {
        if !expected_names.contains(&table.name.to_lowercase()) {
            statements.push(DropTable::new(table.name.as_str()).into());
        }
    }
    statements
}

fn create_table(table: &LiveTable) -> CreateTable {
    let mut statement = CreateTable::new(table.name.as_str());
    for column in &table.columns {
        statement = statement.column(column_def(column));
    }
    statement
}

fn diff_table(expected: &LiveTable, actual: &LiveTable, statements: &mut Vec<Statement>) {
    for column in &expected.columns {
        match actual.column(&column.name) {
            None => {
                statements.push(AddColumn::new(expected.name.as_str(), column_def(column)).into())
            }
            Some(live) if needs_alter(column, live) => {
                statements
                    .push(AlterColumn::new(expected.name.as_str(), column_def(column)).into());
            }
            Some(_) => {}
        }
    }

    for live in &actual.columns {
        if expected.column(&live.name).is_none() {
            statements.push(DropColumn::new(expected.name.as_str(), live.name.as_str()).into());
        }
    }
}

fn needs_alter(expected: &LiveColumn, actual: &LiveColumn) -> bool {
    expected.ty != actual.ty || expected.nullable != actual.nullable
}

fn column_def(column: &LiveColumn) -> ColumnDef {
    let mut def = ColumnDef::new(column.name.as_str(), column.ty.clone());
    if column.primary_key {
        def = def.primary_key();
    }
    if column.identity {
        def = def.identity();
    }
    if !column.nullable {
        def = def.not_null();
    }
    def
}

#[cfg(test)]
mod tests {
    use super::*;
    use ef_orm_query::{Dialect, MssqlDialect, SqlType};

    fn column(name: &str, ty: SqlType, nullable: bool) -> LiveColumn {
        LiveColumn {
            name: name.into(),
            ty,
            nullable,
            primary_key: false,
            identity: false,
        }
    }

    fn table(name: &str, columns: Vec<LiveColumn>) -> LiveTable {
        LiveTable {
            name: name.into(),
            columns,
        }
    }

    fn schema(tables: Vec<LiveTable>) -> LiveSchema {
        LiveSchema { tables }
    }

    fn render(statements: &[Statement]) -> Vec<String> {
        statements
            .iter()
            .map(|statement| MssqlDialect::new().build(statement).expect("renders").0)
            .collect()
    }

    #[test]
    fn creates_missing_tables_with_keys_and_nullability() {
        let id = LiveColumn {
            primary_key: true,
            identity: true,
            ..column("id", SqlType::Int, false)
        };
        let expected = schema(vec![table(
            "users",
            vec![id, column("name", SqlType::NVarChar(50), false)],
        )]);

        let statements = diff(&expected, &LiveSchema::default());
        assert_eq!(
            render(&statements),
            vec![
                "CREATE TABLE [dbo].[users] ([id] INT IDENTITY(1,1) NOT NULL PRIMARY KEY, \
                 [name] NVARCHAR(50) NOT NULL)"
            ]
        );
    }

    #[test]
    fn alters_types_and_drops_and_adds_columns() {
        let expected = schema(vec![table(
            "users",
            vec![
                column("id", SqlType::Int, false),
                column("name", SqlType::NVarChar(100), false),
                column("age", SqlType::Int, true),
            ],
        )]);
        let actual = schema(vec![table(
            "users",
            vec![
                column("id", SqlType::Int, false),
                column("name", SqlType::NVarChar(50), false),
                column("legacy", SqlType::Bit, true),
            ],
        )]);

        let statements = diff(&expected, &actual);
        assert_eq!(
            render(&statements),
            vec![
                "ALTER TABLE [dbo].[users] ALTER COLUMN [name] NVARCHAR(100) NOT NULL",
                "ALTER TABLE [dbo].[users] ADD [age] INT NULL",
                "ALTER TABLE [dbo].[users] DROP COLUMN [legacy]",
            ]
        );
    }

    #[test]
    fn no_statements_when_the_schemas_match() {
        let schema = schema(vec![table(
            "users",
            vec![column("id", SqlType::Int, false)],
        )]);
        assert!(diff(&schema, &schema).is_empty());
    }

    #[test]
    fn drops_tables_absent_from_the_target() {
        let expected = LiveSchema::default();
        let actual = schema(vec![table(
            "legacy",
            vec![column("id", SqlType::Int, false)],
        )]);

        assert_eq!(
            render(&diff(&expected, &actual)),
            vec!["DROP TABLE [dbo].[legacy]"]
        );
    }
}
