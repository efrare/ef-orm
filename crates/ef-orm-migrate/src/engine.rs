//! The migration engine: applies migrations in order and records them in
//! `__ef_migrations`, all inside one transaction per migration.
//!
//! Two sources are supported: [`Migration`] (Rust, AST-generated DDL) and
//! [`SqlMigration`] (SQL files, the CLI deployment mode).

use ef_orm::{Conn, DriverError, OrmError, Row, Tx, Value};
use ef_orm_query::{Dialect, MssqlDialect, Statement};

use crate::history;
use crate::migration::Migration;

/// A migration recorded in `__ef_migrations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMigration {
    /// Ordering key.
    pub version: String,
    /// Description stored when it was applied.
    pub name: String,
}

/// The state of a declared migration relative to the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStatus {
    pub version: String,
    pub name: String,
    /// `true` when the version is present in `__ef_migrations`.
    pub applied: bool,
}

/// A migration loaded from a `*.up.sql` / `*.down.sql` pair (CLI mode).
///
/// The SQL text is user-authored deployment SQL; the engine still runs it
/// inside a transaction and records it in `__ef_migrations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlMigration {
    pub version: String,
    pub name: String,
    pub up_sql: String,
    pub down_sql: String,
}

/// One executable step: AST-generated or verbatim SQL.
enum Step {
    Ast(Statement),
    Sql(String),
}

/// Applies migrations through a connection (sync history table included).
///
/// ```text
/// let mut conn = pool.checkout().await?;
/// let mut migrator = Migrator::new(&mut *conn);
/// migrator.up(&[V1, V2]).await?;
/// ```
pub struct Migrator<'a, C: Conn> {
    conn: &'a mut C,
    dialect: MssqlDialect,
}

impl<'a, C: Conn> Migrator<'a, C> {
    /// Wraps a connection (T-SQL dialect, `dbo` schema).
    pub fn new(conn: &'a mut C) -> Self {
        Self {
            conn,
            dialect: MssqlDialect::default(),
        }
    }

    /// Creates `__ef_migrations` when it does not exist yet.
    pub async fn ensure_history(&mut self) -> Result<(), OrmError> {
        if self.history_exists().await? {
            return Ok(());
        }
        let (sql, params) = self.render(&history::table().into())?;
        self.conn.execute(&sql, &params).await.map_err(db_error)?;
        Ok(())
    }

    /// Every recorded migration, in ascending version order.
    pub async fn applied(&mut self) -> Result<Vec<AppliedMigration>, OrmError> {
        let (sql, params) = self.render(&history::select_applied().into())?;
        let rows = self.conn.query(&sql, &params).await.map_err(db_error)?;

        let mut applied = Vec::with_capacity(rows.len());
        for row in &rows {
            applied.push(AppliedMigration {
                version: required_string(row, "version")?,
                name: required_string(row, "name")?,
            });
        }
        Ok(applied)
    }

    /// Applies every Rust migration not yet in the history.
    pub async fn up(&mut self, migrations: &[Migration]) -> Result<Vec<String>, OrmError> {
        let prepared = migrations
            .iter()
            .map(|migration| {
                (
                    migration.version.to_string(),
                    migration.name.to_string(),
                    migration
                        .up_statements()
                        .into_iter()
                        .map(Step::Ast)
                        .collect(),
                )
            })
            .collect();
        self.apply_pending(prepared).await
    }

    /// Applies every SQL-file migration not yet in the history.
    pub async fn up_sql(&mut self, migrations: &[SqlMigration]) -> Result<Vec<String>, OrmError> {
        let prepared = migrations
            .iter()
            .map(|migration| {
                (
                    migration.version.clone(),
                    migration.name.clone(),
                    vec![Step::Sql(migration.up_sql.clone())],
                )
            })
            .collect();
        self.apply_pending(prepared).await
    }

    /// Rolls back the last `steps` applied Rust migrations (at least one).
    pub async fn down(
        &mut self,
        migrations: &[Migration],
        steps: usize,
    ) -> Result<Vec<String>, OrmError> {
        self.rollback_versions(steps, |version| {
            migrations
                .iter()
                .find(|migration| migration.version == version)
                .map(|migration| {
                    migration
                        .down_statements()
                        .into_iter()
                        .map(Step::Ast)
                        .collect()
                })
        })
        .await
    }

    /// Rolls back the last `steps` applied SQL-file migrations.
    pub async fn down_sql(
        &mut self,
        migrations: &[SqlMigration],
        steps: usize,
    ) -> Result<Vec<String>, OrmError> {
        self.rollback_versions(steps, |version| {
            migrations
                .iter()
                .find(|migration| migration.version == version)
                .map(|migration| vec![Step::Sql(migration.down_sql.clone())])
        })
        .await
    }

    /// Classifies every Rust migration as applied or pending.
    pub async fn status(
        &mut self,
        migrations: &[Migration],
    ) -> Result<Vec<MigrationStatus>, OrmError> {
        let statuses = migrations
            .iter()
            .map(|migration| (migration.version.to_string(), migration.name.to_string()));
        self.classify(statuses).await
    }

    /// Classifies every SQL-file migration as applied or pending.
    pub async fn status_sql(
        &mut self,
        migrations: &[SqlMigration],
    ) -> Result<Vec<MigrationStatus>, OrmError> {
        let statuses = migrations
            .iter()
            .map(|migration| (migration.version.clone(), migration.name.clone()));
        self.classify(statuses).await
    }

    async fn classify<I>(&mut self, entries: I) -> Result<Vec<MigrationStatus>, OrmError>
    where
        I: Iterator<Item = (String, String)>,
    {
        self.ensure_history().await?;
        let applied = self.applied().await?;
        Ok(entries
            .map(|(version, name)| MigrationStatus {
                applied: applied.iter().any(|entry| entry.version == version),
                version,
                name,
            })
            .collect())
    }

    async fn apply_pending(
        &mut self,
        prepared: Vec<(String, String, Vec<Step>)>,
    ) -> Result<Vec<String>, OrmError> {
        self.ensure_history().await?;
        let applied = self.applied().await?;
        let mut ran = Vec::new();

        for (version, name, mut steps) in prepared {
            if applied.iter().any(|entry| entry.version == version) {
                continue;
            }
            steps.push(Step::Ast(history::insert_applied(&version, &name).into()));
            self.apply(&steps).await?;
            ran.push(version);
        }
        Ok(ran)
    }

    async fn rollback_versions<F>(
        &mut self,
        steps: usize,
        resolve: F,
    ) -> Result<Vec<String>, OrmError>
    where
        F: Fn(&str) -> Option<Vec<Step>>,
    {
        self.ensure_history().await?;
        let applied = self.applied().await?;
        let mut rolled_back = Vec::new();

        for entry in applied.iter().rev().take(steps.max(1)) {
            let mut steps = resolve(&entry.version).ok_or_else(|| {
                OrmError::Validation(format!(
                    "applied migration '{}' is not in the provided list",
                    entry.version
                ))
            })?;
            steps.push(Step::Ast(history::delete_applied(&entry.version).into()));
            self.apply(&steps).await?;
            rolled_back.push(entry.version.clone());
        }
        Ok(rolled_back)
    }

    fn render(&self, statement: &Statement) -> Result<(String, Vec<Value>), OrmError> {
        self.dialect
            .build(statement)
            .map_err(|error| OrmError::Database(error.to_string()))
    }

    async fn history_exists(&mut self) -> Result<bool, OrmError> {
        let (sql, params) = self.render(&history::select_exists().into())?;
        let rows = self.conn.query(&sql, &params).await.map_err(db_error)?;
        Ok(!rows.is_empty())
    }

    /// Runs `steps` (already ordered) in a single transaction.
    async fn apply(&mut self, steps: &[Step]) -> Result<(), OrmError> {
        let mut rendered = Vec::with_capacity(steps.len());
        for step in steps {
            match step {
                Step::Ast(statement) => rendered.push(self.render(statement)?),
                Step::Sql(sql) => rendered.push((sql.clone(), Vec::new())),
            }
        }

        let mut tx = self.conn.begin().await.map_err(db_error)?;
        for (sql, params) in &rendered {
            if let Err(error) = tx.execute(sql, params).await {
                let _ = tx.rollback().await;
                return Err(db_error(error));
            }
        }
        tx.commit().await.map_err(db_error)
    }
}

fn required_string(row: &impl Row, column: &str) -> Result<String, OrmError> {
    row.get_by_name::<String>(column)?
        .ok_or_else(|| OrmError::Conversion(format!("__ef_migrations.{column} is NULL")))
}

fn db_error(error: DriverError) -> OrmError {
    OrmError::Database(error.to_string())
}
