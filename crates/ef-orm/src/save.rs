//! [`DbContext::save_changes`] — the unit of work.
//!
//! Staged changes flush inside **one transaction**: statements run through
//! [`Tx`], the transaction commits once, and only then do the write-backs
//! (identity keys) and the tracker's originals update. Any failure rolls back
//! and puts the changes back at the front of the queue, so nothing is lost.

use ef_orm_core::{OrmError, Value};
use ef_orm_driver::{Conn, Driver, Row, Tx};

use crate::change_tracker::{EntityState, StagedChange};
use crate::context::DbContext;
use crate::error::map_driver_error;

/// Counts of the rows written by one [`DbContext::save_changes`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SaveChangesReport {
    /// Rows inserted.
    pub inserted: u64,
    /// Rows updated.
    pub updated: u64,
    /// Rows deleted.
    pub deleted: u64,
}

impl<D: Driver> DbContext<D> {
    /// Applies every staged change atomically and returns what was written.
    ///
    /// ```text
    /// let ticket = ctx.dbset::<User>().insert(user);
    /// ctx.dbset::<User>().update(&other)?;
    /// let report = ctx.save_changes().await?;  // one transaction
    /// let user = ticket.resolve()?;
    /// ```
    ///
    /// - All changes run inside one transaction; a failure rolls back.
    /// - An `UPDATE`/`DELETE` matching zero rows means the row changed
    ///   concurrently → [`OrmError::Conflict`].
    /// - Identity keys are written back into the parked models **after** the
    ///   commit succeeds.
    pub async fn save_changes(&self) -> Result<SaveChangesReport, OrmError> {
        let mut staged = self.tracker().take_staged();
        if staged.is_empty() {
            return Ok(SaveChangesReport::default());
        }

        let generated = match self.flush(&staged).await {
            Ok(generated) => generated,
            Err(error) => {
                self.tracker().restore(staged);
                return Err(error);
            }
        };

        // The transaction committed: fill in the OUTPUT keys, then run the
        // post-commit write-backs (the models are the only thing left to
        // update, and they must never be mutated before a durable commit).
        for (index, key) in generated {
            staged[index].key = Some(key);
        }
        let mut write_back_error = None;
        for change in &mut staged {
            if change.state != EntityState::Inserted {
                continue;
            }
            if let (Some(mut write_back), Some(key)) =
                (change.write_back.take(), change.key.clone())
            {
                if let Err(error) = write_back(key) {
                    write_back_error.get_or_insert(error);
                }
            }
        }

        let report = count(&staged);
        self.tracker().finish(&staged);

        match write_back_error {
            Some(error) => Err(error),
            None => Ok(report),
        }
    }

    /// Runs every staged statement inside one transaction and commits.
    ///
    /// Returns the identity keys read from `OUTPUT INSERTED` (with the index
    /// of the change that produced them) — they are applied only after the
    /// commit, so a rollback never leaves a stale key behind.
    async fn flush(&self, staged: &[StagedChange]) -> Result<Vec<(usize, Value)>, OrmError> {
        let mut conn = self.pool().checkout().await.map_err(map_driver_error)?;
        let mut tx = conn.begin().await.map_err(map_driver_error)?;
        let mut generated = Vec::new();

        for (index, change) in staged.iter().enumerate() {
            let step = async {
                let (sql, params) = self.build(&change.stmt)?;
                run(&mut tx, change, &sql, &params).await
            }
            .await;

            match step {
                Ok(Some(key)) => generated.push((index, key)),
                Ok(None) => {}
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(error);
                }
            }
        }

        tx.commit().await.map_err(map_driver_error)?;
        Ok(generated)
    }
}

/// Runs one staged statement, returning a generated key for identity inserts.
async fn run<T: Tx>(
    tx: &mut T,
    change: &StagedChange,
    sql: &str,
    params: &[Value],
) -> Result<Option<Value>, OrmError> {
    match change.state {
        // No key staged: the `INSERT` carries `OUTPUT INSERTED.<pk>`.
        EntityState::Inserted if change.key.is_none() => {
            let rows = tx.query(sql, params).await.map_err(map_driver_error)?;
            let row = rows.first().ok_or_else(|| {
                OrmError::Database(format!(
                    "INSERT into {} returned no OUTPUT row for {}",
                    change.place.1, change.pk
                ))
            })?;
            Ok(Some(row.value_of(change.pk)?.clone()))
        }
        EntityState::Inserted => {
            tx.execute(sql, params).await.map_err(map_driver_error)?;
            Ok(None)
        }
        EntityState::Modified | EntityState::Deleted => {
            let affected = tx.execute(sql, params).await.map_err(map_driver_error)?;
            if affected == 0 {
                return Err(OrmError::Conflict(format!(
                    "{} changed concurrently ({} = {:?} no longer matches)",
                    change.place.1, change.pk, change.key
                )));
            }
            Ok(None)
        }
        EntityState::Unchanged => Ok(None),
    }
}

fn count(staged: &[StagedChange]) -> SaveChangesReport {
    let mut report = SaveChangesReport::default();
    for change in staged {
        match change.state {
            EntityState::Inserted => report.inserted += 1,
            EntityState::Modified => report.updated += 1,
            EntityState::Deleted => report.deleted += 1,
            EntityState::Unchanged => {}
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{NoopDriver, ScriptedDriver};
    use ef_orm_core::{Entity, Model};
    use ef_orm_driver::{Pool, RowData};
    use std::sync::Arc;

    struct User {
        id: i32,
        name: String,
    }

    impl Entity for User {
        const TABLE: &'static str = "users";
        const COLUMNS: &'static [&'static str] = &["id", "name"];
        const PRIMARY_KEY: &'static str = "id";
        const PRIMARY_KEY_AUTO: bool = true;
        type Id = i32;
        type Columns = ();
        fn columns() -> Self::Columns {}
    }

    impl Model for User {
        fn to_values(&self) -> Vec<(&'static str, Value)> {
            vec![("name", self.name.clone().into())]
        }
        fn id(&self) -> Self::Id {
            self.id
        }
        fn set_id(&mut self, id: Value) -> Result<(), OrmError> {
            self.id = match id {
                Value::Int(id) => id as i32,
                other => return Err(OrmError::Conversion(format!("id: {other:?}"))),
            };
            Ok(())
        }
    }

    fn ctx_with<D: Driver>(driver: D) -> DbContext<D> {
        DbContext::new(Pool::new(driver, Default::default()))
    }

    fn row(id: i32, name: &str) -> RowData {
        RowData::new(
            Arc::from(vec!["id".to_string(), "name".to_string()]),
            vec![Value::Int(id.into()), Value::Str(name.into())],
        )
    }

    #[tokio::test]
    async fn an_empty_queue_saves_nothing() {
        let ctx = ctx_with(NoopDriver);

        let report = ctx.save_changes().await.expect("nothing to do");
        assert_eq!(report, SaveChangesReport::default());
    }

    #[tokio::test]
    async fn inserts_commit_and_write_the_key_back() {
        let ctx = ctx_with(ScriptedDriver::new(
            vec![RowData::new(
                Arc::from(vec!["id".to_string()]),
                vec![Value::Int(99)],
            )],
            1,
        ));

        let ticket = ctx.dbset::<User>().insert(User {
            id: 0,
            name: "ada".into(),
        });
        let report = ctx.save_changes().await.expect("saves");

        assert_eq!(report.inserted, 1);
        assert_eq!(ticket.key(), Some(Value::Int(99)));
        assert_eq!(ticket.resolve().expect("resolves").id, 99);
    }

    #[tokio::test]
    async fn a_zero_row_update_is_a_conflict_and_the_change_is_restored() {
        let ctx = ctx_with(ScriptedDriver::new(Vec::new(), 0));
        ctx.track::<User>(&row(7, "old")).expect("tracks");
        ctx.dbset::<User>()
            .update(&User {
                id: 7,
                name: "new".into(),
            })
            .expect("stages");

        let error = ctx.save_changes().await.expect_err("conflict");
        assert!(matches!(error, OrmError::Conflict(_)));
        assert_eq!(ctx.tracker().staged_len(), 1, "restored for a retry");
    }

    #[tokio::test]
    async fn updates_and_deletes_report_their_counts() {
        let ctx = ctx_with(ScriptedDriver::new(Vec::new(), 1));
        ctx.track::<User>(&row(7, "old")).expect("tracks");
        ctx.dbset::<User>()
            .update(&User {
                id: 7,
                name: "new".into(),
            })
            .expect("stages");
        ctx.track::<User>(&row(8, "gone")).expect("tracks");
        ctx.dbset::<User>().delete(8).expect("stages");

        let report = ctx.save_changes().await.expect("saves");
        assert_eq!(
            report,
            SaveChangesReport {
                inserted: 0,
                updated: 1,
                deleted: 1,
            }
        );
    }
}
