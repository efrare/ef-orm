//! Snapshot tests: AST → T-SQL for `INSERT` / `UPDATE` / `DELETE`.

use ef_orm_query::{
    DeleteStatement, Dialect, Expr, InsertStatement, MssqlDialect, QueryError, Statement,
    UpdateStatement, Value,
};

fn render(stmt: impl Into<Statement>) -> Result<(String, Vec<Value>), QueryError> {
    MssqlDialect::new().build(&stmt.into())
}

// --- INSERT ---------------------------------------------------------------

#[test]
fn insert_single_row_binds_values() {
    let stmt = InsertStatement::new("users")
        .columns(["name", "age"])
        .values([Value::from("Ada"), Value::from(36)]);
    let (sql, params) = render(stmt).expect("valid insert");
    assert_eq!(
        sql,
        "INSERT INTO [dbo].[users] ([name], [age]) VALUES (@P1, @P2)"
    );
    assert_eq!(params, vec![Value::Str("Ada".into()), Value::Int(36)]);
}

#[test]
fn insert_multi_row_with_identity_output() {
    let stmt = InsertStatement::new("users")
        .column("name")
        .values(["Ada"])
        .values(["Grace"])
        .output("id");
    let (sql, params) = render(stmt).expect("valid insert");
    assert_eq!(
        sql,
        "INSERT INTO [dbo].[users] ([name]) OUTPUT INSERTED.[id] \
         VALUES (@P1), (@P2)"
    );
    assert_eq!(
        params,
        vec![Value::Str("Ada".into()), Value::Str("Grace".into())]
    );
}

#[test]
fn insert_rejects_row_wider_than_columns() {
    let stmt = InsertStatement::new("users")
        .column("name")
        .values([Value::from("Ada"), Value::from(36)]);
    let err = render(stmt).expect_err("width mismatch");
    assert!(matches!(err, QueryError::InvalidStatement(_)), "{err:?}");
}

#[test]
fn insert_rejects_missing_columns_or_rows() {
    let no_cols = InsertStatement::new("users");
    assert!(render(no_cols).is_err());

    let no_rows = InsertStatement::new("users").column("name");
    assert!(render(no_rows).is_err());
}

// --- UPDATE ---------------------------------------------------------------

#[test]
fn update_sets_values_output_and_where() {
    let stmt = UpdateStatement::new("users")
        .set("name", "Grace")
        .set("age", 37)
        .and_where(Expr::col("id").eq(1))
        .output("id");
    let (sql, params) = render(stmt).expect("valid update");
    assert_eq!(
        sql,
        "UPDATE [dbo].[users] SET [name] = @P1, [age] = @P2 \
         OUTPUT INSERTED.[id] WHERE [id] = @P3"
    );
    assert_eq!(
        params,
        vec![Value::Str("Grace".into()), Value::Int(37), Value::Int(1)]
    );
}

#[test]
fn update_without_assignments_is_rejected() {
    let stmt = UpdateStatement::new("users").and_where(Expr::col("id").eq(1));
    assert!(render(stmt).is_err());
}

#[test]
fn update_without_where_updates_everything() {
    let stmt = UpdateStatement::new("users").set("flag", true);
    let (sql, _) = render(stmt).expect("valid update");
    assert_eq!(sql, "UPDATE [dbo].[users] SET [flag] = @P1");
}

// --- DELETE ---------------------------------------------------------------

#[test]
fn delete_with_output_and_where() {
    let stmt = DeleteStatement::new("users")
        .and_where(Expr::col("id").eq(1))
        .output("id");
    let (sql, params) = render(stmt).expect("valid delete");
    assert_eq!(
        sql,
        "DELETE FROM [dbo].[users] OUTPUT DELETED.[id] WHERE [id] = @P1"
    );
    assert_eq!(params, vec![Value::Int(1)]);
}

#[test]
fn delete_without_where_is_allowed() {
    let (sql, _) = render(DeleteStatement::new("users")).expect("valid delete");
    assert_eq!(sql, "DELETE FROM [dbo].[users]");
}
