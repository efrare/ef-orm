//! [`StagedInsert`] — the ticket handed back by a staged `INSERT`.
//!
//! `DbSet::insert` takes the model **by value** and parks it in a shared
//! `Shared<T>`: the ticket keeps one handle, the tracker's write-back closure
//! keeps another. After `save_changes` commits, the closure writes the
//! server-generated key back into the same model, and the ticket can
//! [`resolve`](StagedInsert::resolve) it.

use std::sync::{Arc, Mutex};

use ef_orm_core::{Model, OrmError, Value};

use crate::change_tracker::WriteBack;

/// The parked model plus the key once the save committed.
struct Shared<T> {
    model: T,
    saved: Option<Value>,
}

/// Handle to an `INSERT` staged by [`DbSet::insert`](crate::DbSet::insert).
///
/// ```text
/// let ticket = ctx.dbset::<User>().insert(user); // moved in, nothing written
/// ctx.save_changes().await?;                     // INSERT + COMMIT
/// let user = ticket.resolve()?;                  // back, with its new id
/// ```
pub struct StagedInsert<T> {
    shared: Arc<Mutex<Shared<T>>>,
}

impl<T: Model> StagedInsert<T> {
    /// Parks the model and builds the write-back the tracker runs post-commit.
    pub(crate) fn new(model: T) -> (Self, WriteBack) {
        let shared = Arc::new(Mutex::new(Shared { model, saved: None }));
        let ticket = Self {
            shared: Arc::clone(&shared),
        };
        let write_back: WriteBack = Box::new(move |key: Value| {
            let mut shared = shared
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            shared.model.set_id(key.clone())?;
            shared.saved = Some(key);
            Ok(())
        });
        (ticket, write_back)
    }

    /// The key once the save committed (`None` until then).
    pub fn key(&self) -> Option<Value> {
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .saved
            .clone()
    }

    /// Takes the model back — only after a successful `save_changes`.
    ///
    /// While the tracker still owns the write-back (the row is awaiting a
    /// save, or the save failed and the change was restored) the handle is
    /// not unique, so this reports [`OrmError::Validation`] instead of
    /// handing back a model that was never persisted.
    pub fn resolve(self) -> Result<T, OrmError> {
        let shared = Arc::try_unwrap(self.shared).map_err(|_| {
            OrmError::Validation(
                "resolve() before save_changes(): the insert is still staged".into(),
            )
        })?;
        let shared = shared
            .into_inner()
            .map_err(|_| OrmError::Validation("staged insert state was poisoned".into()))?;
        if shared.saved.is_none() {
            return Err(OrmError::Validation(
                "resolve() before save_changes(): the insert was not saved".into(),
            ));
        }
        Ok(shared.model)
    }
}

#[cfg(test)]
mod tests {
    use super::StagedInsert;
    use ef_orm_core::{Entity, Model, OrmError, Value};

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
            match id {
                Value::Int(id) => {
                    self.id = id as i32;
                    Ok(())
                }
                other => Err(OrmError::Conversion(format!("id: {other:?}"))),
            }
        }
    }

    fn user() -> User {
        User {
            id: 0,
            name: "ada".into(),
        }
    }

    #[test]
    fn resolve_fails_while_the_insert_is_staged() {
        // The write-back handle still exists (as it does inside the tracker).
        let (ticket, _write_back) = StagedInsert::new(user());

        assert!(ticket.key().is_none(), "no key before the save");
        assert!(matches!(ticket.resolve(), Err(OrmError::Validation(_))));
    }

    #[test]
    fn resolve_returns_the_model_once_the_write_back_ran() {
        let (ticket, mut write_back) = StagedInsert::new(user());
        write_back(Value::Int(42)).expect("write back");
        drop(write_back);

        assert_eq!(ticket.key(), Some(Value::Int(42)));
        let user = ticket.resolve().expect("saved insert resolves");
        assert_eq!(user.id, 42);
        assert_eq!(user.name, "ada");
    }

    #[test]
    fn resolve_rejects_a_ticket_dropped_without_a_write_back() {
        // A handle released without a save (e.g. the staged change was
        // replaced) must not hand back an unpersisted model.
        let (ticket, write_back) = StagedInsert::new(user());
        drop(write_back);

        assert!(matches!(ticket.resolve(), Err(OrmError::Validation(_))));
    }
}
