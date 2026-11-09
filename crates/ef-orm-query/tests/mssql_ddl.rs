//! Snapshot tests: DDL AST → T-SQL (`CREATE`/`ALTER`/`DROP`).

use ef_orm_query::{
    AddColumn, AddForeignKey, AlterColumn, ColumnDef, CreateIndex, CreateTable, DefaultValue,
    Dialect, DropColumn, DropConstraint, DropIndex, DropTable, MssqlDialect, QueryError, RefAction,
    SqlType, Statement, Value,
};

fn render(stmt: impl Into<Statement>) -> Result<(String, Vec<Value>), QueryError> {
    MssqlDialect::new().build(&stmt.into())
}

fn users() -> CreateTable {
    CreateTable::new("users")
        .column(ColumnDef::new("id", SqlType::Int).identity().primary_key())
        .column(ColumnDef::new("name", SqlType::NVarChar(100)).not_null())
        .column(
            ColumnDef::new("created_at", SqlType::DateTime2)
                .not_null()
                .default(DefaultValue::SysUtcDateTime),
        )
}

#[test]
fn create_table_renders_columns_types_and_constraints() {
    let (sql, params) = render(users()).expect("valid create table");

    assert_eq!(
        sql,
        "CREATE TABLE [dbo].[users] (\
         [id] INT IDENTITY(1,1) NOT NULL PRIMARY KEY, \
         [name] NVARCHAR(100) NOT NULL, \
         [created_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME())"
    );
    assert!(params.is_empty(), "DDL binds no parameters");
}

#[test]
fn defaults_inline_literals_with_tsql_escaping() {
    let stmt = CreateTable::new("flags").column(
        ColumnDef::new("label", SqlType::NVarChar(50))
            .default(DefaultValue::Literal(Value::Str("it's".into()))),
    );
    let (sql, _) = render(stmt).expect("valid default");

    assert_eq!(
        sql,
        "CREATE TABLE [dbo].[flags] ([label] NVARCHAR(50) NULL DEFAULT N'it''s')"
    );
}

#[test]
fn numeric_and_binary_defaults_render_as_literals() {
    let stmt = CreateTable::new("t")
        .column(ColumnDef::new("n", SqlType::Int).default(DefaultValue::Literal(Value::Int(7))))
        .column(
            ColumnDef::new("b", SqlType::Binary(2))
                .default(DefaultValue::Literal(Value::Bytes(vec![0x0A, 0xFF]))),
        );
    let (sql, _) = render(stmt).expect("valid defaults");

    assert_eq!(
        sql,
        "CREATE TABLE [dbo].[t] ([n] INT NULL DEFAULT 7, [b] BINARY(2) NULL DEFAULT 0x0AFF)"
    );
}

#[test]
fn create_table_rejects_an_empty_column_list() {
    let error = render(CreateTable::new("empty")).expect_err("must fail");
    assert_eq!(error, QueryError::invalid("CREATE TABLE without columns"));
}

#[test]
fn drop_table_qualifies_the_schema() {
    let (sql, _) = render(DropTable::new("users")).expect("valid drop");
    assert_eq!(sql, "DROP TABLE [dbo].[users]");
}

#[test]
fn add_column_preserves_the_column_definition() {
    let stmt = AddColumn::new(
        "users",
        ColumnDef::new("email", SqlType::NVarChar(200)).not_null(),
    );
    let (sql, _) = render(stmt).expect("valid add column");
    assert_eq!(
        sql,
        "ALTER TABLE [dbo].[users] ADD [email] NVARCHAR(200) NOT NULL"
    );
}

#[test]
fn drop_column_names_the_column() {
    let (sql, _) = render(DropColumn::new("users", "email")).expect("valid drop column");
    assert_eq!(sql, "ALTER TABLE [dbo].[users] DROP COLUMN [email]");
}

#[test]
fn alter_column_changes_type_and_nullability_only() {
    // IDENTITY and DEFAULT are not representable in `ALTER COLUMN`.
    let column = ColumnDef::new("name", SqlType::NVarChar(50))
        .not_null()
        .default(DefaultValue::Literal(Value::Str("x".into())));
    let (sql, _) = render(AlterColumn::new("users", column)).expect("valid alter");
    assert_eq!(
        sql,
        "ALTER TABLE [dbo].[users] ALTER COLUMN [name] NVARCHAR(50) NOT NULL"
    );
}

#[test]
fn create_unique_index_lists_its_columns() {
    let stmt = CreateIndex::new("ix_users_name", "users")
        .column("name")
        .unique();
    let (sql, _) = render(stmt).expect("valid index");
    assert_eq!(
        sql,
        "CREATE UNIQUE INDEX [ix_users_name] ON [dbo].[users] ([name])"
    );
}

#[test]
fn create_index_rejects_an_empty_column_list() {
    let error = render(CreateIndex::new("ix", "users")).expect_err("must fail");
    assert_eq!(error, QueryError::invalid("CREATE INDEX without columns"));
}

#[test]
fn drop_index_qualifies_the_table() {
    let (sql, _) = render(DropIndex::new("ix_users_name", "users")).expect("valid drop index");
    assert_eq!(sql, "DROP INDEX [ix_users_name] ON [dbo].[users]");
}

#[test]
fn foreign_key_renders_columns_references_and_actions() {
    let stmt = AddForeignKey::new("fk_posts_author", "posts", "users")
        .column("author_id")
        .references("id")
        .on_delete(RefAction::Cascade)
        .on_update(RefAction::NoAction);
    let (sql, _) = render(stmt).expect("valid foreign key");

    assert_eq!(
        sql,
        "ALTER TABLE [dbo].[posts] ADD CONSTRAINT [fk_posts_author] \
         FOREIGN KEY ([author_id]) REFERENCES [dbo].[users] ([id]) \
         ON DELETE CASCADE ON UPDATE NO ACTION"
    );
}

#[test]
fn foreign_key_rejects_mismatched_column_counts() {
    let stmt = AddForeignKey::new("fk", "posts", "users")
        .column("a")
        .column("b")
        .references("id");
    let error = render(stmt).expect_err("must fail");
    assert_eq!(
        error,
        QueryError::invalid("FOREIGN KEY has 2 columns for 1 referenced columns")
    );
}

#[test]
fn drop_constraint_names_the_constraint() {
    let (sql, _) = render(DropConstraint::new("posts", "fk_posts_author")).expect("valid drop");
    assert_eq!(
        sql,
        "ALTER TABLE [dbo].[posts] DROP CONSTRAINT [fk_posts_author]"
    );
}

#[test]
fn decimal_and_max_types_render_their_lengths() {
    let stmt = CreateTable::new("t")
        .column(ColumnDef::new(
            "price",
            SqlType::Decimal {
                precision: 18,
                scale: 2,
            },
        ))
        .column(ColumnDef::new("body", SqlType::NVarCharMax))
        .column(ColumnDef::new("raw", SqlType::VarBinaryMax));
    let (sql, _) = render(stmt).expect("valid types");
    assert_eq!(
        sql,
        "CREATE TABLE [dbo].[t] (\
         [price] DECIMAL(18,2) NULL, [body] NVARCHAR(MAX) NULL, [raw] VARBINARY(MAX) NULL)"
    );
}
