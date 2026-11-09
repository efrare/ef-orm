use crate::change_tracker::EntityState;
use crate::context::DbContext;
use crate::test_support::{NoopDriver, ScriptedDriver};
use ef_orm_core::{Entity, Model, OrmError, Value};
use ef_orm_driver::{FromRow, Pool, Row};
use ef_orm_query::{Dialect, MssqlDialect, Statement};
use std::sync::Arc;

struct User {
    id: i32,
    name: String,
    age: i32,
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
        vec![("name", self.name.clone().into()), ("age", self.age.into())]
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

impl FromRow for User {
    fn from_row<R: Row>(row: &R) -> Result<Self, OrmError> {
        Ok(Self {
            id: row
                .get_by_name("id")?
                .ok_or_else(|| OrmError::Conversion("id is NULL".into()))?,
            name: row
                .get_by_name("name")?
                .ok_or_else(|| OrmError::Conversion("name is NULL".into()))?,
            age: row
                .get_by_name("age")?
                .ok_or_else(|| OrmError::Conversion("age is NULL".into()))?,
        })
    }
}

struct Tag {
    code: String,
    label: String,
}

impl Entity for Tag {
    const TABLE: &'static str = "tags";
    const COLUMNS: &'static [&'static str] = &["code", "label"];
    const PRIMARY_KEY: &'static str = "code";
    type Id = String;
    type Columns = ();
    fn columns() -> Self::Columns {}
}

impl Model for Tag {
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

fn ctx() -> DbContext<NoopDriver> {
    DbContext::new(Pool::new(NoopDriver, Default::default()))
}

fn row(id: i32, name: &str, age: i32) -> ef_orm_driver::RowData {
    ef_orm_driver::RowData::new(
        Arc::from(vec!["id".into(), "name".into(), "age".into()]),
        vec![
            Value::Int(id.into()),
            Value::Str(name.into()),
            Value::Int(age.into()),
        ],
    )
}

fn render(stmt: &Statement) -> (String, Vec<Value>) {
    MssqlDialect::default().build(stmt).expect("renders")
}

#[test]
fn insert_stages_an_output_read_for_identity_keys() {
    let ctx = ctx();
    let _ticket = ctx.dbset::<User>().insert(User {
        id: 0,
        name: "ada".into(),
        age: 36,
    });

    let staged = ctx.tracker().take_staged();
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].state, EntityState::Inserted);
    assert!(staged[0].key.is_none(), "identity key comes from OUTPUT");
    assert!(staged[0].write_back.is_some());
    let (sql, _) = render(&staged[0].stmt);
    assert_eq!(
        sql,
        "INSERT INTO [dbo].[users] ([name], [age]) \
         OUTPUT INSERTED.[id] VALUES (@P1, @P2)"
    );
}

#[test]
fn natural_key_inserts_stage_their_key() {
    let ctx = ctx();
    let _ticket = ctx.dbset::<Tag>().insert(Tag {
        code: "BR".into(),
        label: "Brazil".into(),
    });

    let staged = ctx.tracker().take_staged();
    assert_eq!(staged[0].key, Some(Value::Str("BR".into())));
    let (sql, _) = render(&staged[0].stmt);
    assert_eq!(
        sql,
        "INSERT INTO [dbo].[tags] ([code], [label]) VALUES (@P1, @P2)"
    );
}

#[test]
fn update_after_a_read_stages_the_diff_guarded_by_originals() {
    let ctx = ctx();
    ctx.track::<User>(&row(7, "old", 36)).expect("tracks");

    ctx.dbset::<User>()
        .update(&User {
            id: 7,
            name: "new".into(),
            age: 36,
        })
        .expect("stages");

    let staged = ctx.tracker().take_staged();
    assert_eq!(staged[0].state, EntityState::Modified);
    let (sql, params) = render(&staged[0].stmt);
    assert_eq!(
        sql,
        "UPDATE [dbo].[users] SET [name] = @P1 \
         WHERE [id] = @P2 AND [name] = @P3 AND [age] = @P4"
    );
    assert_eq!(
        params,
        vec![
            Value::Str("new".into()),
            Value::Int(7),
            Value::Str("old".into()),
            Value::Int(36),
        ]
    );
}

#[test]
fn an_unchanged_update_cancels_the_pending_change() {
    let ctx = ctx();
    ctx.track::<User>(&row(7, "same", 36)).expect("tracks");

    ctx.dbset::<User>()
        .update(&User {
            id: 7,
            name: "same".into(),
            age: 36,
        })
        .expect("no-op update");

    assert_eq!(ctx.tracker().staged_len(), 0);
}

#[test]
fn delete_stages_a_guarded_statement() {
    let ctx = ctx();
    ctx.track::<User>(&row(9, "old", 20)).expect("tracks");

    ctx.dbset::<User>().delete(9).expect("stages");

    let staged = ctx.tracker().take_staged();
    assert_eq!(staged[0].state, EntityState::Deleted);
    let (sql, _) = render(&staged[0].stmt);
    assert_eq!(
        sql,
        "DELETE FROM [dbo].[users] WHERE [id] = @P1 \
         AND [name] = @P2 AND [age] = @P3"
    );
}

#[test]
fn deleting_a_staged_insert_cancels_it_instead_of_staging_a_delete() {
    let ctx = ctx();
    let _ticket = ctx.dbset::<Tag>().insert(Tag {
        code: "BR".into(),
        label: "Brazil".into(),
    });

    ctx.dbset::<Tag>().delete("BR".into()).expect("cancels");

    assert_eq!(ctx.tracker().staged_len(), 0, "the row was never written");
}

#[tokio::test]
async fn insert_many_bulk_inserts_every_model() {
    let ctx = DbContext::new(Pool::new(
        ScriptedDriver::new(Vec::new(), 0),
        Default::default(),
    ));

    let inserted = ctx
        .dbset::<User>()
        .insert_many(&[
            User {
                id: 0,
                name: "ada".into(),
                age: 36,
            },
            User {
                id: 0,
                name: "grace".into(),
                age: 45,
            },
        ])
        .await
        .expect("bulk insert");

    assert_eq!(inserted, 2);
}

#[tokio::test]
async fn find_in_batches_keys_into_one_query() {
    let ctx = DbContext::new(Pool::new(
        ScriptedDriver::new(vec![row(1, "ada", 36), row(2, "grace", 45)], 0),
        Default::default(),
    ));
    let id = crate::ColumnRef::<i32>::new("id");

    let found = ctx
        .dbset::<User>()
        .find_in(id, [1, 2])
        .await
        .expect("loads");

    assert_eq!(found.len(), 2);
    assert_eq!(found[1].name, "grace");
}
