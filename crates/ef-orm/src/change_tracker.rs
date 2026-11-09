//! EF-style change tracking: staged changes + per-row original snapshots.
//!
//! The tracker lives inside [`DbContext`](crate::DbContext) behind a `Mutex`:
//! reads (`find`/`select`) snapshot each row's values as **originals**;
//! `insert`/`update`/`delete` stage a prepared [`Statement`]; and
//! [`DbContext::save_changes`](crate::DbContext::save_changes) flushes the
//! staged list inside one transaction, refreshing originals on success.

use ef_orm_core::{OrmError, Value};
use ef_orm_query::Statement;

/// Lifecycle state of a staged entity (EF Core-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityState {
    /// Staged for `INSERT`; its key is read from `OUTPUT INSERTED` on save.
    Inserted,
    /// Staged for `UPDATE` (diff against the tracked originals).
    Modified,
    /// Staged for `DELETE`.
    Deleted,
    /// Present in the database and tracked, with no pending change.
    Unchanged,
}

/// Schema + table identifying which entity a row belongs to.
pub(crate) type Place = (Option<&'static str>, &'static str);

/// A captured row: what the database looked like when it was last read.
pub(crate) struct TrackedRow {
    pub place: Place,
    pub key: Value,
    pub values: Vec<(&'static str, Value)>,
}

/// Fills a staged insert's model with the server-generated key after commit.
pub(crate) type WriteBack = Box<dyn FnMut(Value) -> Result<(), OrmError> + Send>;

/// One pending change, fully prepared at stage time.
pub(crate) struct StagedChange {
    pub state: EntityState,
    /// Statement already built (SET diff + optimistic-concurrency `WHERE`).
    pub stmt: Statement,
    pub place: Place,
    pub pk: &'static str,
    /// `None` for identity inserts — filled by the `OUTPUT` read on save.
    pub key: Option<Value>,
    /// Current column values (they become the originals after a save).
    pub current: Vec<(&'static str, Value)>,
    /// Identity insert only: writes the generated key back into the model.
    pub write_back: Option<WriteBack>,
}

/// Pending changes plus the originals of every tracked row.
pub(crate) struct ChangeTracker {
    staged: Vec<StagedChange>,
    originals: Vec<TrackedRow>,
}

impl ChangeTracker {
    pub fn new() -> Self {
        Self {
            staged: Vec::new(),
            originals: Vec::new(),
        }
    }

    /// Snapshots a row read from the database (upsert by place + key).
    ///
    /// `Value` is not `Eq`/`Hash`, so lookup is a linear scan — sessions hold
    /// a handful of tracked rows, and correctness beats hashing here.
    pub fn track(&mut self, place: Place, key: Value, values: Vec<(&'static str, Value)>) {
        match self
            .originals
            .iter_mut()
            .find(|row| row.place == place && row.key == key)
        {
            Some(row) => row.values = values,
            None => self.originals.push(TrackedRow { place, key, values }),
        }
    }

    /// The snapshot of a tracked row, if it was read through a tracked query.
    pub fn originals_of(&self, place: Place, key: &Value) -> Option<&[(&'static str, Value)]> {
        self.originals
            .iter()
            .find(|row| row.place == place && row.key == *key)
            .map(|row| row.values.as_slice())
    }

    /// Queues a change. A keyed change replaces any earlier stage for the
    /// same row (last call wins); identity inserts (key `None`) always stack.
    pub fn stage(&mut self, change: StagedChange) {
        if let Some(key) = &change.key {
            self.unstage(change.place, key);
        }
        self.staged.push(change);
    }

    /// Drops the pending change for one row (used when an update's diff
    /// turns out to be empty after a previous stage).
    pub fn unstage(&mut self, place: Place, key: &Value) {
        self.staged
            .retain(|change| !(change.place == place && change.key.as_ref() == Some(key)));
    }

    /// Number of changes waiting for `save_changes` (tests inspect the queue).
    #[cfg(test)]
    pub fn staged_len(&self) -> usize {
        self.staged.len()
    }

    /// True when the pending change for this row is an `INSERT`.
    ///
    /// `delete` uses it to cancel a row that was staged but never written
    /// (delete of an added entity is a no-op, like EF).
    pub fn has_staged_insert(&self, place: Place, key: &Value) -> bool {
        self.staged.iter().any(|change| {
            change.place == place
                && change.key.as_ref() == Some(key)
                && change.state == EntityState::Inserted
        })
    }

    /// Moves the staged list out for flushing (new stages during the flush
    /// land in the now-empty list and are untouched by [`Self::restore`]).
    pub fn take_staged(&mut self) -> Vec<StagedChange> {
        std::mem::take(&mut self.staged)
    }

    /// Puts a failed flush back at the front of the queue, in order.
    pub fn restore(&mut self, mut changes: Vec<StagedChange>) {
        changes.append(&mut self.staged);
        self.staged = changes;
    }

    /// Applies a successful flush: inserts/updates refresh their originals,
    /// deletes stop being tracked. Called after commit with the flushed list.
    pub fn finish(&mut self, saved: &[StagedChange]) {
        for change in saved {
            let Some(key) = &change.key else {
                continue; // identity insert whose OUTPUT was never read
            };
            match change.state {
                EntityState::Deleted => self
                    .originals
                    .retain(|row| !(row.place == change.place && row.key == *key)),
                EntityState::Inserted | EntityState::Modified => {
                    self.track(change.place, key.clone(), change.current.clone())
                }
                EntityState::Unchanged => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ChangeTracker, EntityState, Place, StagedChange};
    use ef_orm_core::Value;
    use ef_orm_query::{DeleteStatement, Statement, UpdateStatement};

    const USERS: Place = (None, "users");

    fn staged(state: EntityState, key: Option<Value>) -> StagedChange {
        StagedChange {
            state,
            stmt: Statement::Delete(DeleteStatement::new("users")),
            place: USERS,
            pk: "id",
            key,
            current: vec![("name", Value::Str("ana".into()))],
            write_back: None,
        }
    }

    #[test]
    fn tracking_snapshots_by_place_and_key() {
        let mut tracker = ChangeTracker::new();
        tracker.track(USERS, Value::Int(1), vec![("name", Value::Str("a".into()))]);
        tracker.track(USERS, Value::Int(1), vec![("name", Value::Str("b".into()))]);
        tracker.track(USERS, Value::Int(2), vec![("name", Value::Str("c".into()))]);

        // Re-reading refreshes (upsert), never duplicates.
        assert_eq!(
            tracker.originals_of(USERS, &Value::Int(1)),
            Some(&[("name", Value::Str("b".into()))][..])
        );
        assert_eq!(tracker.originals_of(USERS, &Value::Int(9)), None);
    }

    #[test]
    fn keyed_stages_replace_older_ones_and_unstage_cancels() {
        let mut tracker = ChangeTracker::new();

        tracker.stage(staged(EntityState::Modified, Some(Value::Int(1))));
        tracker.stage(staged(EntityState::Deleted, Some(Value::Int(1))));
        assert_eq!(tracker.staged_len(), 1, "last stage for the row wins");

        tracker.stage(staged(EntityState::Inserted, None));
        tracker.stage(staged(EntityState::Inserted, None));
        assert_eq!(tracker.staged_len(), 3, "identity inserts always stack");

        tracker.unstage(USERS, &Value::Int(1));
        assert_eq!(tracker.staged_len(), 2, "only keyed entries match");
        tracker.unstage(USERS, &Value::Int(1));
        assert_eq!(tracker.staged_len(), 2, "unstage of absent key is a no-op");
    }

    #[test]
    fn failed_flush_is_restored_in_front_and_finish_refreshes_originals() {
        let mut tracker = ChangeTracker::new();
        tracker.track(
            USERS,
            Value::Int(1),
            vec![("name", Value::Str("old".into()))],
        );
        tracker.stage(staged(EntityState::Modified, Some(Value::Int(1))));

        // A new stage arriving while flushing must survive the restore.
        let flushed = tracker.take_staged();
        tracker.stage(staged(EntityState::Modified, Some(Value::Int(2))));
        tracker.restore(flushed);
        assert_eq!(
            tracker.staged_len(),
            2,
            "flushed change returns to the front"
        );

        // Success: the update's current values become the new originals.
        let saved = tracker.take_staged();
        tracker.finish(&saved);
        assert_eq!(
            tracker.originals_of(USERS, &Value::Int(1)),
            Some(&[("name", Value::Str("ana".into()))][..])
        );

        // Deletes stop being tracked.
        let mut del = staged(EntityState::Deleted, Some(Value::Int(2)));
        del.key = Some(Value::Int(1));
        tracker.finish(std::slice::from_ref(&del));
        assert_eq!(tracker.originals_of(USERS, &Value::Int(1)), None);
    }

    #[test]
    fn update_statements_are_prepared_and_opaque_here() {
        // The tracker stores whatever the builders produced — statement
        // construction itself is covered in `statements` tests.
        let change = StagedChange {
            state: EntityState::Modified,
            stmt: Statement::Update(UpdateStatement::new("users").set("name", "x")),
            place: USERS,
            pk: "id",
            key: Some(Value::Int(4)),
            current: vec![],
            write_back: None,
        };
        assert_eq!(change.state, EntityState::Modified);
    }
}
