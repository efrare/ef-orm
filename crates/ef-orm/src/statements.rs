//! Pure statement builders: entity metadata → query AST.
//!
//! Everything here is side-effect free and returns `ef-orm-query` statements,
//! so the whole mapping is unit-testable with SQL snapshots (no database).

use ef_orm_core::{Entity, Model, Value};
use ef_orm_query::{Expr, InsertStatement, SelectStatement, Table};

/// The query `Table` for an entity (schema defaults to the dialect's `dbo`).
pub(crate) fn table_of<T: Entity>() -> Table {
    match T::SCHEMA {
        Some(schema) => Table::with_schema(schema, T::TABLE),
        None => Table::new(T::TABLE),
    }
}

/// `SELECT [cols] FROM …` — the projection every read starts from.
pub(crate) fn select_of<T: Entity>() -> SelectStatement {
    SelectStatement::new(table_of::<T>()).columns(T::COLUMNS.iter().copied())
}

/// `INSERT INTO … (cols) VALUES (…) [OUTPUT INSERTED.[pk]]`.
///
/// Columns and values come from [`Model::to_values`] (auto keys omitted);
/// `OUTPUT` is added only for identity keys, so the facade can write the
/// generated value back with [`Model::set_id`].
pub(crate) fn insert_of<T: Model>(model: &T) -> InsertStatement {
    let values = model.to_values();
    let mut stmt =
        InsertStatement::new(table_of::<T>()).columns(values.iter().map(|(name, _)| *name));
    stmt = stmt.values(values.into_iter().map(|(_, value)| value));
    if T::PRIMARY_KEY_AUTO {
        stmt = stmt.output(T::PRIMARY_KEY);
    }
    stmt
}

/// `SELECT … WHERE pk = @P1` limited to one row (the `find` lookup).
pub(crate) fn find_of<T: Entity>(key: &Value) -> SelectStatement {
    select_of::<T>()
        .and_where(Expr::col(T::PRIMARY_KEY).eq(Expr::val(key.clone())))
        .limit(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ef_orm_core::OrmError;
    use ef_orm_driver::FromValue;
    use ef_orm_query::{Dialect, MssqlDialect, OrderDir, Page, Statement};

    struct User {
        id: i32,
        name: String,
        age: i32,
    }

    struct NaturalKey {
        code: String,
        label: String,
    }

    impl Entity for User {
        const TABLE: &'static str = "users";
        const COLUMNS: &'static [&'static str] = &["id", "name", "age"];
        const PRIMARY_KEY: &'static str = "id";
        const PRIMARY_KEY_AUTO: bool = true;
        type Id = i32;
        type Columns = ();
        fn columns() -> Self::Columns {}
    }

    impl Model for User {
        fn to_values(&self) -> Vec<(&'static str, Value)> {
            vec![
                // identity key omitted
                ("name", self.name.clone().into()),
                ("age", self.age.into()),
            ]
        }
        fn id(&self) -> Self::Id {
            self.id
        }
        fn set_id(&mut self, id: Value) -> Result<(), OrmError> {
            // Same conversion the derive emits for an `i32` key.
            self.id = <i32 as FromValue>::from_value(&id)?;
            Ok(())
        }
    }

    impl Entity for NaturalKey {
        const TABLE: &'static str = "codes";
        const SCHEMA: Option<&'static str> = Some("sales");
        const COLUMNS: &'static [&'static str] = &["code", "label"];
        const PRIMARY_KEY: &'static str = "code";
        type Id = String;
        type Columns = ();
        fn columns() -> Self::Columns {}
    }

    impl Model for NaturalKey {
        fn to_values(&self) -> Vec<(&'static str, Value)> {
            vec![
                ("code", self.code.clone().into()),
                ("label", self.label.clone().into()),
            ]
        }
        fn id(&self) -> Self::Id {
            self.code.clone()
        }
        fn set_id(&mut self, _id: Value) -> Result<(), OrmError> {
            Ok(())
        }
    }

    fn render(stmt: impl Into<Statement>) -> (String, Vec<Value>) {
        MssqlDialect::default()
            .build(&stmt.into())
            .expect("statement renders")
    }

    fn user() -> User {
        User {
            id: 0,
            name: "ada".into(),
            age: 36,
        }
    }

    #[test]
    fn insert_omits_the_identity_key_and_adds_output() {
        let (sql, params) = render(insert_of(&user()));

        assert_eq!(
            sql,
            "INSERT INTO [dbo].[users] ([name], [age]) \
             OUTPUT INSERTED.[id] VALUES (@P1, @P2)"
        );
        assert_eq!(params, vec![Value::Str("ada".into()), Value::Int(36)]);
    }

    #[test]
    fn insert_keeps_a_natural_key_and_needs_no_output() {
        let key = NaturalKey {
            code: "BR-1".into(),
            label: "Brazil".into(),
        };
        let (sql, params) = render(insert_of(&key));

        assert_eq!(
            sql,
            "INSERT INTO [sales].[codes] ([code], [label]) VALUES (@P1, @P2)"
        );
        assert_eq!(
            params,
            vec![Value::Str("BR-1".into()), Value::Str("Brazil".into())]
        );
    }

    #[test]
    fn find_projects_all_columns_and_limits_to_one_row() {
        let (sql, params) = render(find_of::<User>(&Value::Int(7)));

        assert_eq!(
            sql,
            "SELECT TOP (@P1) [id], [name], [age] FROM [dbo].[users] WHERE [id] = @P2"
        );
        assert_eq!(params, vec![Value::Int(1), Value::Int(7)]);
    }

    #[test]
    fn select_starts_from_the_full_projection() {
        let (sql, params) = render(
            select_of::<User>()
                .and_where(Expr::col("age").gt(Expr::val(18)))
                .order_by("age", OrderDir::Desc)
                .page(Page::new(2, 10)),
        );

        assert_eq!(
            sql,
            "SELECT [id], [name], [age] FROM [dbo].[users] WHERE [age] > @P1 \
             ORDER BY [age] DESC OFFSET @P2 ROWS FETCH NEXT @P3 ROWS ONLY"
        );
        assert_eq!(params, vec![Value::Int(18), Value::Int(10), Value::Int(10)]);
    }
}
