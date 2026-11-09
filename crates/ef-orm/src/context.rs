//! [`DbContext`] — the session of work users hold onto.

use std::sync::{Mutex, MutexGuard};

use ef_orm_core::{Entity, Model, OrmError, Value};
use ef_orm_driver::{Conn, Driver, Pool, PooledConn, Row};
use ef_orm_query::{Dialect, MssqlDialect, Statement};

use crate::change_tracker::{ChangeTracker, StagedChange};
use crate::error::map_driver_error;
use crate::raw_sql::RawSql;
use crate::set::DbSet;

/// A session over a pool of connections: entry point for every operation.
///
/// Owns the [`Pool`] (built by the caller with `ef-orm-driver-tds`'s
/// `TdsDriver`, so this crate stays driver-agnostic), the SQL dialect and the
/// change tracker. Reads (`find`/`select`) snapshot their rows for optimistic
/// concurrency; writes are staged and a [`save_changes`](Self::save_changes)
/// flushes them inside one transaction.
///
/// ```text
/// let ctx = DbContext::new(Pool::new(TdsDriver::new(config), PoolOptions::default()));
/// let ticket = ctx.dbset::<User>().insert(user);
/// ctx.save_changes().await?;
/// let user = ticket.resolve()?;
/// ```
pub struct DbContext<D: Driver> {
    pool: Pool<D>,
    dialect: MssqlDialect,
    tracker: Mutex<ChangeTracker>,
}

impl<D: Driver> DbContext<D> {
    /// Wraps an already-built pool (default dialect: `dbo` schema).
    pub fn new(pool: Pool<D>) -> Self {
        Self {
            pool,
            dialect: MssqlDialect::default(),
            tracker: Mutex::new(ChangeTracker::new()),
        }
    }

    /// Typed access to the table mapped by `T`.
    pub fn dbset<T: Model>(&self) -> DbSet<'_, D, T> {
        DbSet::new(self)
    }

    /// Runs a raw statement (DDL, admin commands) and returns affected rows.
    ///
    /// This is the escape hatch used by examples/migrations to create tables;
    /// the typed API covers DML. For row-returning SQL use
    /// [`raw_sql`](Self::raw_sql).
    pub async fn execute(&self, sql: &str, params: &[Value]) -> Result<u64, OrmError> {
        let mut conn = self.pool.checkout().await.map_err(map_driver_error)?;
        conn.execute(sql, params).await.map_err(map_driver_error)
    }

    /// Starts a raw SQL statement (placeholders `@P1`, `@P2`, …).
    ///
    /// ```text
    /// ctx.raw_sql("SELECT COUNT(*) FROM [dbo].[users] WHERE [age] > @P1")
    ///     .param(18)
    ///     .fetch_one::<Count>()
    ///     .await?;
    /// ```
    pub fn raw_sql(&self, sql: impl Into<String>) -> RawSql<'_, D> {
        RawSql::new(self, sql.into())
    }

    /// Checks out a connection for advanced use (e.g. streaming rows through
    /// [`Select::stream`](crate::Select::stream)).
    pub async fn checkout(&self) -> Result<PooledConn<'_, D>, OrmError> {
        self.pool.checkout().await.map_err(map_driver_error)
    }

    /// Renders a statement with this session's dialect.
    pub(crate) fn build(&self, stmt: &Statement) -> Result<(String, Vec<Value>), OrmError> {
        self.dialect
            .build(stmt)
            .map_err(|error| OrmError::Database(error.to_string()))
    }

    /// The underlying pool (used by `DbSet`/`Select` to check out connections).
    pub(crate) fn pool(&self) -> &Pool<D> {
        &self.pool
    }

    /// Locks the change tracker (short, synchronous critical sections only —
    /// never held across an `.await`).
    pub(crate) fn tracker(&self) -> MutexGuard<'_, ChangeTracker> {
        self.tracker
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A quoted, schema-qualified table name (bulk copy interpolates it).
    pub(crate) fn qualified_table(&self, schema: Option<&str>, table: &str) -> String {
        let schema = schema
            .or_else(|| self.dialect.default_schema())
            .unwrap_or("dbo");
        format!(
            "{}.{}",
            self.dialect.quote_ident(schema),
            self.dialect.quote_ident(table)
        )
    }

    /// Queues a prepared change for the next `save_changes`.
    pub(crate) fn stage(&self, change: StagedChange) {
        self.tracker().stage(change);
    }

    /// Snapshots a row just read as this entity's tracked originals.
    ///
    /// The snapshot holds every projected column (including the key); the
    /// concurrency guard skips the key when building predicates.
    pub(crate) fn track<T: Entity>(&self, row: &impl Row) -> Result<(), OrmError> {
        let place = (T::SCHEMA, T::TABLE);
        let key = row.value_of(T::PRIMARY_KEY)?.clone();
        let values = T::COLUMNS
            .iter()
            .map(|name| row.value_of(name).map(|value| (*name, value.clone())))
            .collect::<Result<Vec<_>, OrmError>>()?;
        self.tracker().track(place, key, values);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::NoopDriver;

    #[test]
    fn default_dialect_qualifies_tables_with_dbo() {
        let ctx = DbContext::new(Pool::new(NoopDriver, Default::default()));
        let (sql, params) = ctx
            .build(&Statement::Select(ef_orm_query::SelectStatement::new("t")))
            .expect("renders");

        assert_eq!(sql, "SELECT * FROM [dbo].[t]");
        assert!(params.is_empty());
    }
}
