//! [`DbSet`] â€” typed access to one entity's table.
//!
//! Reads (`find`, `select`) hit the database immediately and **track** the
//! rows they return. Writes (`insert`, `update`, `delete`) are *staged*:
//! they build the statement synchronously and apply nothing until
//! [`DbContext::save_changes`](crate::DbContext::save_changes) flushes them
//! inside one transaction.

use std::marker::PhantomData;

use ef_orm_core::{Column, Model, OrmError, Value};
use ef_orm_driver::{Conn, Driver, FromRow};
use ef_orm_query::{Expr, Statement};

use crate::change_tracker::{EntityState, StagedChange};
use crate::concurrency::{delete_guarded, update_tracked};
use crate::context::DbContext;
use crate::error::map_driver_error;
use crate::select::Select;
use crate::staged::StagedInsert;
use crate::statements;

/// Typed access to the table mapped by `T` (obtained from
/// [`DbContext::dbset`](crate::DbContext::dbset)).
pub struct DbSet<'a, D: Driver, T: Model> {
    ctx: &'a DbContext<D>,
    _model: PhantomData<T>,
}

impl<'a, D: Driver, T: Model> DbSet<'a, D, T> {
    pub(crate) fn new(ctx: &'a DbContext<D>) -> Self {
        Self {
            ctx,
            _model: PhantomData,
        }
    }

    /// Stages an `INSERT`; nothing hits the database until `save_changes`.
    ///
    /// The model moves into the returned [`StagedInsert`] â€” identity keys are
    /// filled from `OUTPUT INSERTED` during the save and written back into it,
    /// so the model comes back via [`StagedInsert::resolve`] once committed.
    pub fn insert(&self, model: T) -> StagedInsert<T> {
        let place = (T::SCHEMA, T::TABLE);
        let stmt = Statement::Insert(statements::insert_of(&model));
        let key: Option<Value> = if T::PRIMARY_KEY_AUTO {
            None // filled from OUTPUT INSERTED on save
        } else {
            Some(model.id().into())
        };
        let current = model.to_values();
        let (ticket, write_back) = StagedInsert::new(model);

        self.ctx.stage(StagedChange {
            state: EntityState::Inserted,
            stmt,
            place,
            pk: T::PRIMARY_KEY,
            key,
            current,
            write_back: Some(write_back),
        });
        ticket
    }

    /// Stages an `UPDATE` with the diff against the tracked originals.
    ///
    /// The statement is guarded by every original non-key value, so a
    /// concurrent modification surfaces as [`OrmError::Conflict`] on
    /// `save_changes`. A model with no changes cancels any pending change for
    /// the same row; a row that was never read degrades to a key-only update.
    pub fn update(&self, model: &T) -> Result<(), OrmError> {
        let place = (T::SCHEMA, T::TABLE);
        let key: Value = model.id().into();
        let mut tracker = self.ctx.tracker();
        let originals = tracker
            .originals_of(place, &key)
            .map(|values| values.to_vec())
            .unwrap_or_default();

        match update_tracked(model, &originals) {
            Some(stmt) => tracker.stage(StagedChange {
                state: EntityState::Modified,
                stmt: Statement::Update(stmt),
                place,
                pk: T::PRIMARY_KEY,
                key: Some(key),
                current: model.to_values(),
                write_back: None,
            }),
            None => tracker.unstage(place, &key),
        }
        Ok(())
    }

    /// Bulk-inserts `models` in one batch (Tiberius `BulkLoadRequest`).
    ///
    /// No `OUTPUT` is read: identity keys stay at their in-memory value and
    /// are **not** written back. Use [`insert`](Self::insert) +
    /// `save_changes` when the generated keys are needed. Returns the
    /// inserted row count.
    pub async fn insert_many(&self, models: &[T]) -> Result<u64, OrmError> {
        let Some(first) = models.first() else {
            return Ok(0);
        };
        let columns: Vec<&str> = first.to_values().iter().map(|(name, _)| *name).collect();

        let mut rows = Vec::with_capacity(models.len());
        for model in models {
            let values = model.to_values();
            if values.len() != columns.len() {
                return Err(OrmError::Validation(
                    "insert_many: models expose different column sets".into(),
                ));
            }
            rows.push(values.into_iter().map(|(_, value)| value).collect());
        }

        let table = self.ctx.qualified_table(T::SCHEMA, T::TABLE);
        let mut conn = self.ctx.pool().checkout().await.map_err(map_driver_error)?;
        conn.bulk_insert(&table, &columns, &rows)
            .await
            .map_err(map_driver_error)
    }

    /// Stages a `DELETE` by primary key, guarded by the tracked originals.
    ///
    /// Deleting a row whose `INSERT` is still staged just cancels the insert;
    /// otherwise the `DELETE` runs on `save_changes` and a missing row
    /// (zero affected) reports [`OrmError::Conflict`].
    pub fn delete(&self, id: T::Id) -> Result<(), OrmError> {
        let key: Value = id.into();
        let place = (T::SCHEMA, T::TABLE);
        let mut tracker = self.ctx.tracker();

        if tracker.has_staged_insert(place, &key) {
            tracker.unstage(place, &key);
            return Ok(());
        }

        let originals = tracker
            .originals_of(place, &key)
            .map(|values| values.to_vec())
            .unwrap_or_default();
        tracker.stage(StagedChange {
            state: EntityState::Deleted,
            stmt: delete_guarded::<T>(&key, &originals),
            place,
            pk: T::PRIMARY_KEY,
            key: Some(key),
            current: Vec::new(),
            write_back: None,
        });
        Ok(())
    }
}

impl<'a, D: Driver, T: Model + FromRow> DbSet<'a, D, T> {
    /// Starts a fluent, tracked `SELECT` over the entity's columns.
    pub fn select(&self) -> Select<'a, D, T> {
        Select::new(self.ctx)
    }

    /// Loads every row whose `column` is in `keys`, in **one** query.
    ///
    /// This is the batched half of an eager load: fetch the parents, collect
    /// their keys, then load all children at once (instead of one query per
    /// parent). Group the result in Rust; rows are tracked as usual.
    ///
    /// ```text
    /// let authors = ctx.dbset::<Author>().select().all().await?;
    /// let posts = ctx.dbset::<Post>()
    ///     .find_in(Post::columns().author_code, authors.iter().map(|a| a.code.clone()))
    ///     .await?; // one query for every author
    /// ```
    pub async fn find_in<K>(
        &self,
        column: K,
        keys: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Result<Vec<T>, OrmError>
    where
        K: Column,
    {
        let keys: Vec<Value> = keys.into_iter().map(Into::into).collect();
        self.select()
            .filter(|_| Expr::col(column.name()).is_in(keys))
            .all()
            .await
    }

    /// Loads one row by primary key and **tracks** it; [`OrmError::NotFound`]
    /// when absent.
    pub async fn find(&self, id: T::Id) -> Result<T, OrmError> {
        let key: Value = id.into();
        let (sql, params) = self
            .ctx
            .build(&Statement::Select(statements::find_of::<T>(&key)))?;
        let mut conn = self.ctx.pool().checkout().await.map_err(map_driver_error)?;
        let rows = conn.query(&sql, &params).await.map_err(map_driver_error)?;

        let row = rows.first().ok_or_else(|| {
            OrmError::NotFound(format!("{} where {} = {key:?}", T::TABLE, T::PRIMARY_KEY))
        })?;
        let model = T::from_row(row)?;
        self.ctx.track::<T>(row)?;
        Ok(model)
    }
}
