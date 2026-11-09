//! Snapshot tests: AST → T-SQL for `SELECT`.
//!
//! Every divergence from the rules in `references/mssql-dialect.md`
//! must fail here.

use ef_orm_query::{
    Dialect, Expr, MssqlDialect, OrderDir, Page, SelectStatement, Statement, Value,
};

fn render(stmt: impl Into<Statement>) -> (String, Vec<Value>) {
    MssqlDialect::new()
        .build(&stmt.into())
        .expect("statement should render")
}

#[test]
fn select_star_uses_default_schema() {
    let (sql, params) = render(SelectStatement::new("users"));
    assert_eq!(sql, "SELECT * FROM [dbo].[users]");
    assert!(params.is_empty());
}

#[test]
fn explicit_schema_is_respected() {
    let stmt = SelectStatement::new(ef_orm_query::Table::with_schema("sales", "orders"));
    let (sql, _) = render(stmt);
    assert_eq!(sql, "SELECT * FROM [sales].[orders]");
}

#[test]
fn bracket_quoting_escapes_closing_bracket() {
    let (sql, _) = render(SelectStatement::new("weird]name"));
    assert_eq!(sql, "SELECT * FROM [dbo].[weird]]name]");
}

#[test]
fn columns_and_where_binds_parameters() {
    let stmt = SelectStatement::new("users")
        .columns(["id", "name"])
        .and_where(Expr::col("age").ge(18));
    let (sql, params) = render(stmt);
    assert_eq!(
        sql,
        "SELECT [id], [name] FROM [dbo].[users] WHERE [age] >= @P1"
    );
    assert_eq!(params, vec![Value::Int(18)]);
}

#[test]
fn limit_without_offset_renders_top_with_param_first() {
    let stmt = SelectStatement::new("users")
        .column("id")
        .and_where(Expr::col("active").eq(true))
        .limit(5);
    let (sql, params) = render(stmt);
    assert_eq!(
        sql,
        "SELECT TOP (@P1) [id] FROM [dbo].[users] WHERE [active] = @P2"
    );
    assert_eq!(params, vec![Value::Int(5), Value::Bool(true)]);
}

#[test]
fn page_renders_offset_and_fetch() {
    let stmt = SelectStatement::new("users")
        .order_by("id", OrderDir::Desc)
        .page(Page::new(3, 10));
    let (sql, params) = render(stmt);
    assert_eq!(
        sql,
        "SELECT * FROM [dbo].[users] ORDER BY [id] DESC \
         OFFSET @P1 ROWS FETCH NEXT @P2 ROWS ONLY"
    );
    // page 3 × size 10 ⇒ skip 20, take 10
    assert_eq!(params, vec![Value::Int(20), Value::Int(10)]);
}

#[test]
fn offset_without_order_injects_order_by_null() {
    let (sql, params) = render(SelectStatement::new("users").offset(5));
    assert_eq!(
        sql,
        "SELECT * FROM [dbo].[users] ORDER BY (SELECT NULL) OFFSET @P1 ROWS"
    );
    assert_eq!(params, vec![Value::Int(5)]);
}

#[test]
fn nested_or_group_is_parenthesized() {
    let filter = Expr::col("a")
        .eq(1)
        .or(Expr::col("b").eq(2))
        .and(Expr::col("c").like("x%"));
    let (sql, params) = render(SelectStatement::new("t").and_where(filter));
    assert_eq!(
        sql,
        "SELECT * FROM [dbo].[t] \
         WHERE ([a] = @P1 OR [b] = @P2) AND [c] LIKE @P3"
    );
    assert_eq!(
        params,
        vec![Value::Int(1), Value::Int(2), Value::Str("x%".into())]
    );
}

#[test]
fn null_and_not_checks() {
    let stmt = SelectStatement::new("users")
        .and_where(Expr::col("email").is_not_null())
        .and_where(Expr::col("deleted_at").is_null());
    let (sql, _) = render(stmt);
    assert_eq!(
        sql,
        "SELECT * FROM [dbo].[users] \
         WHERE [email] IS NOT NULL AND [deleted_at] IS NULL"
    );
}

#[test]
fn not_wraps_its_child() {
    let (sql, _) = render(SelectStatement::new("t").and_where(!Expr::col("a").eq(1)));
    assert_eq!(sql, "SELECT * FROM [dbo].[t] WHERE NOT ([a] = @P1)");
}

#[test]
fn in_list_binds_each_value() {
    let stmt = SelectStatement::new("t").and_where(Expr::col("id").is_in([1, 2, 3]));
    let (sql, params) = render(stmt);
    assert_eq!(sql, "SELECT * FROM [dbo].[t] WHERE [id] IN (@P1, @P2, @P3)");
    assert_eq!(params, vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
}

#[test]
fn empty_in_list_becomes_constant() {
    let stmt = SelectStatement::new("t").and_where(Expr::col("id").is_in(Vec::<i32>::new()));
    let (sql, params) = render(stmt);
    assert_eq!(sql, "SELECT * FROM [dbo].[t] WHERE 1 = 0");
    assert!(params.is_empty());

    let stmt = SelectStatement::new("t").and_where(Expr::col("id").is_not_in(Vec::<i32>::new()));
    let (sql, _) = render(stmt);
    assert_eq!(sql, "SELECT * FROM [dbo].[t] WHERE 1 = 1");
}

#[test]
fn multiple_order_by_terms() {
    let stmt = SelectStatement::new("t")
        .order_by("name", OrderDir::Asc)
        .order_by("id", OrderDir::Desc);
    let (sql, _) = render(stmt);
    assert_eq!(
        sql,
        "SELECT * FROM [dbo].[t] ORDER BY [name] ASC, [id] DESC"
    );
}
