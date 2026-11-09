//! Optimistic-concurrency statement builders.
//!
//! Phase 4 turns `UPDATE`/`DELETE` into guarded statements: besides the
//! primary key, the `WHERE` repeats **every non-key original** captured when
//! the row was read. If a concurrent writer touched the row first, zero rows
//! match and the facade reports [`OrmError::Conflict`](ef_orm_core::OrmError).
//!
//! Pure functions over the tracker's snapshot — unit-testable as SQL strings.

use ef_orm_core::{Entity, Model, Value};
use ef_orm_query::{DeleteStatement, Expr, Statement, UpdateStatement};

use crate::statements::table_of;

/// `UPDATE … SET <changed cols> WHERE pk = @k AND <all originals>`.
///
/// The `SET` only carries columns whose value differs from the snapshot;
/// when nothing differs there is nothing to stage, so this returns `None`.
/// An empty snapshot (row read without tracking) degrades to the plain
/// key-filtered update of the earlier phase.
pub(crate) fn update_tracked<T: Model>(
    model: &T,
    originals: &[(&'static str, Value)],
) -> Option<UpdateStatement> {
    let mut stmt = UpdateStatement::new(table_of::<T>());
    let mut changed = false;
    for (name, value) in model.to_values() {
        if name == T::PRIMARY_KEY {
            continue;
        }
        let unchanged = originals.iter().any(|(old, v)| *old == name && *v == value);
        if !unchanged {
            stmt = stmt.set(name, value);
            changed = true;
        }
    }
    if !changed {
        return None;
    }

    stmt = stmt.and_where(Expr::col(T::PRIMARY_KEY).eq(Expr::val(model.id())));
    if let Some(guard) = guard_of(originals, T::PRIMARY_KEY) {
        stmt = stmt.and_where(guard);
    }
    Some(stmt)
}

/// `DELETE FROM … WHERE pk = @k AND <all originals>` (same guard as update).
pub(crate) fn delete_guarded<T: Entity>(
    key: &Value,
    originals: &[(&'static str, Value)],
) -> Statement {
    let mut stmt = DeleteStatement::new(table_of::<T>())
        .and_where(Expr::col(T::PRIMARY_KEY).eq(Expr::val(key.clone())));
    if let Some(guard) = guard_of(originals, T::PRIMARY_KEY) {
        stmt = stmt.and_where(guard);
    }
    Statement::Delete(stmt)
}

/// `col1 = @p AND col2 IS NULL AND …` over the non-key originals.
///
/// `Null` originals become `IS NULL` (equality with `NULL` never matches in
/// SQL). Returns `None` when there is nothing to guard besides the key.
fn guard_of(originals: &[(&'static str, Value)], pk: &str) -> Option<Expr> {
    let mut guard: Option<Expr> = None;
    for (name, value) in originals.iter().filter(|(name, _)| *name != pk) {
        let pred = match value {
            Value::Null => Expr::col(*name).is_null(),
            other => Expr::col(*name).eq(Expr::val(other.clone())),
        };
        guard = Some(match guard {
            Some(left) => left.and(pred),
            None => pred,
        });
    }
    guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use ef_orm_core::OrmError;
    use ef_orm_query::{Dialect, MssqlDialect};

    struct User {
        id: i32,
        name: String,
        age: i32,
        nickname: Option<String>,
    }

    impl Entity for User {
        const TABLE: &'static str = "users";
        const COLUMNS: &'static [&'static str] = &["id", "name", "age", "nickname"];
        const PRIMARY_KEY: &'static str = "id";
        const PRIMARY_KEY_AUTO: bool = true;
        type Id = i32;
        type Columns = ();
        fn columns() -> Self::Columns {}
    }

    impl Model for User {
        fn to_values(&self) -> Vec<(&'static str, Value)> {
            vec![
                ("name", self.name.clone().into()),
                ("age", self.age.into()),
                ("nickname", self.nickname.clone().into()),
            ]
        }
        fn id(&self) -> Self::Id {
            self.id
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
            id: 7,
            name: "ada".into(),
            age: 36,
            nickname: None,
        }
    }

    fn snapshot() -> Vec<(&'static str, Value)> {
        vec![
            ("id", Value::Int(7)),
            ("name", Value::Str("old".into())),
            ("age", Value::Int(36)),
            ("nickname", Value::Null),
        ]
    }

    #[test]
    fn unchanged_snapshot_stages_nothing() {
        let mut same = user();
        same.name = "old".into();
        assert!(
            update_tracked(&same, &snapshot()).is_none(),
            "a diff-less update must not be staged"
        );
    }

    #[test]
    fn update_sets_only_the_diff_and_guards_every_original() {
        // `age` keeps its value → not in SET; `nickname` changed
        // None → Some, while the original was NULL → IS NULL in the guard.
        let mut changed = user();
        changed.nickname = Some("ada".into());
        let (sql, params) = render(update_tracked(&changed, &snapshot()).expect("diff exists"));

        assert_eq!(
            sql,
            "UPDATE [dbo].[users] SET [name] = @P1, [nickname] = @P2 \
             WHERE [id] = @P3 AND [name] = @P4 AND [age] = @P5 AND [nickname] IS NULL"
        );
        assert_eq!(
            params,
            vec![
                Value::Str("ada".into()),
                Value::Str("ada".into()),
                Value::Int(7),
                Value::Str("old".into()),
                Value::Int(36),
            ]
        );
    }

    #[test]
    fn empty_snapshot_degrades_to_a_key_filtered_update() {
        let (sql, params) = render(update_tracked(&user(), &[]).expect("diff exists"));

        assert_eq!(
            sql,
            "UPDATE [dbo].[users] SET [name] = @P1, [age] = @P2, [nickname] = @P3 \
             WHERE [id] = @P4"
        );
        assert_eq!(params.len(), 4, "only the key filters an untracked row");
    }

    #[test]
    fn delete_carries_the_same_concurrency_guard() {
        let (sql, params) = render(delete_guarded::<User>(&Value::Int(7), &snapshot()));

        assert_eq!(
            sql,
            "DELETE FROM [dbo].[users] WHERE [id] = @P1 \
             AND [name] = @P2 AND [age] = @P3 AND [nickname] IS NULL"
        );
        assert_eq!(
            params,
            vec![Value::Int(7), Value::Str("old".into()), Value::Int(36),]
        );

        // Without a snapshot the delete filters by key only.
        let (plain, plain_params) = render(delete_guarded::<User>(&Value::Int(7), &[]));
        assert_eq!(plain, "DELETE FROM [dbo].[users] WHERE [id] = @P1");
        assert_eq!(plain_params, vec![Value::Int(7)]);
    }
}
