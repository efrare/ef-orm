//! Raw SQL escape hatch: [`DbContext::raw_sql`].
//!
//! The typed API covers DML; this is the door for engine-specific SQL
//! (hints, CTEs, admin commands). Parameters are bound positionally, so the
//! caller writes T-SQL placeholders (`@P1`, `@P2`, …).

use ef_orm_core::{OrmError, Value};
use ef_orm_driver::{Conn, Driver, FromRow, RowData};

use crate::context::DbContext;
use crate::error::map_driver_error;

/// A raw statement plus its bound parameters.
///
/// ```text
/// let rows = ctx.raw_sql("SELECT [id], [name] FROM [dbo].[users] WHERE [age] >= @P1")
///     .param(18)
///     .fetch_all::<UserRef>()
///     .await?;
/// ```
pub struct RawSql<'a, D: Driver> {
    ctx: &'a DbContext<D>,
    sql: String,
    params: Vec<Value>,
}

impl<'a, D: Driver> RawSql<'a, D> {
    pub(crate) fn new(ctx: &'a DbContext<D>, sql: String) -> Self {
        Self {
            ctx,
            sql,
            params: Vec::new(),
        }
    }

    /// Binds the next positional parameter (`@P1`, `@P2`, … in call order).
    pub fn param(mut self, value: impl Into<Value>) -> Self {
        self.params.push(value.into());
        self
    }

    /// Runs the statement and returns the affected row count.
    pub async fn execute(self) -> Result<u64, OrmError> {
        let mut conn = self.ctx.pool().checkout().await.map_err(map_driver_error)?;
        conn.execute(&self.sql, &self.params)
            .await
            .map_err(map_driver_error)
    }

    /// Runs the statement and returns every row (first result set).
    pub async fn rows(self) -> Result<Vec<RowData>, OrmError> {
        let mut conn = self.ctx.pool().checkout().await.map_err(map_driver_error)?;
        conn.query(&self.sql, &self.params)
            .await
            .map_err(map_driver_error)
    }

    /// Maps every row to `T` (columns are read **by name**, so a projection
    /// can be narrower than the entity).
    pub async fn fetch_all<T: FromRow>(self) -> Result<Vec<T>, OrmError> {
        self.rows()
            .await?
            .iter()
            .map(T::from_row)
            .collect::<Result<Vec<_>, OrmError>>()
    }

    /// Maps the first row to `T`, or `None` when the statement returned none.
    pub async fn fetch_one<T: FromRow>(self) -> Result<Option<T>, OrmError> {
        self.rows().await?.first().map(T::from_row).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{NoopDriver, ScriptedDriver};
    use ef_orm_driver::{Pool, Row};
    use std::sync::Arc;

    #[derive(Debug, PartialEq)]
    struct Pick {
        id: i32,
        name: String,
    }

    impl FromRow for Pick {
        fn from_row<R: Row>(row: &R) -> Result<Self, OrmError> {
            let id = row
                .get_by_name::<i32>("id")?
                .ok_or_else(|| OrmError::Conversion("id is NULL".into()))?;
            let name = row
                .get_by_name::<String>("name")?
                .ok_or_else(|| OrmError::Conversion("name is NULL".into()))?;
            Ok(Self { id, name })
        }
    }

    fn row(id: i32, name: &str) -> RowData {
        RowData::new(
            Arc::from(vec!["id".to_string(), "name".to_string()]),
            vec![Value::Int(id.into()), Value::Str(name.into())],
        )
    }

    fn ctx_with<D: Driver>(driver: D) -> DbContext<D> {
        DbContext::new(Pool::new(driver, Default::default()))
    }

    #[tokio::test]
    async fn fetch_all_maps_rows_by_name() {
        let ctx = ctx_with(ScriptedDriver::new(vec![row(1, "ada"), row(2, "grace")], 0));

        let picks = ctx
            .raw_sql("SELECT [id], [name] FROM [dbo].[users] WHERE [age] >= @P1")
            .param(18)
            .fetch_all::<Pick>()
            .await
            .expect("maps");

        assert_eq!(
            picks,
            vec![
                Pick {
                    id: 1,
                    name: "ada".into()
                },
                Pick {
                    id: 2,
                    name: "grace".into()
                }
            ]
        );
    }

    #[tokio::test]
    async fn fetch_one_returns_none_for_an_empty_result() {
        let ctx = ctx_with(NoopDriver);
        let pick = ctx
            .raw_sql("SELECT 1")
            .fetch_one::<Pick>()
            .await
            .expect("no rows is fine");
        assert!(pick.is_none());
    }

    #[tokio::test]
    async fn execute_reports_the_affected_rows() {
        let ctx = ctx_with(ScriptedDriver::new(Vec::new(), 3));
        let affected = ctx
            .raw_sql("UPDATE [dbo].[users] SET [name] = @P1")
            .param("x")
            .execute()
            .await
            .expect("runs");
        assert_eq!(affected, 3);
    }
}
