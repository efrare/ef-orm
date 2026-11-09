//! Phase-4 proof: change tracking and the unit of work against a real SQL
//! Server.
//!
//! - `insert`/`update`/`delete` are **staged**; `save_changes` flushes them
//!   in one transaction and reports what was written.
//! - `find`/`select` snapshot their rows; an `UPDATE`/`DELETE` is guarded by
//!   every original non-key value, so a concurrent writer surfaces as
//!   `OrmError::Conflict` and the whole transaction rolls back.
//! - A failed save restores the staged changes, so re-reading and re-staging
//!   the affected rows recovers.
//!
//! Credentials never live in the code: they come from environment variables
//! (same names as `hello-crud`).
//!
//! Run with: `cargo run -p ef-orm-examples --example change-tracking`

use ef_orm::{DbContext, Entity, Model, OrmError, Pool, PoolOptions, Value};
use ef_orm_driver_tds::{Config, TdsDriver};
use std::env;

const CREATE_TABLE: &str = "
IF OBJECT_ID(N'dbo.ef_orm_tracked', N'U') IS NULL
    CREATE TABLE [dbo].[ef_orm_tracked] (
        [id] INT IDENTITY(1,1) NOT NULL PRIMARY KEY,
        [text] NVARCHAR(100) NOT NULL,
        [version] INT NOT NULL
    );";

#[derive(Entity, Model, Debug)]
#[orm(table = "ef_orm_tracked")]
struct Note {
    #[orm(primary_key, auto)]
    id: i32,
    text: String,
    version: i32,
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let password = env::var("EF_ORM_TEST_DB_PASSWORD").map_err(|_| {
        "EF_ORM_TEST_DB_PASSWORD is not set — export it first, \
         e.g. export EF_ORM_TEST_DB_PASSWORD='...'"
    })?;

    let config = Config {
        host: env_or("EF_ORM_TEST_DB_HOST", "192.168.0.10"),
        port: env_or("EF_ORM_TEST_DB_PORT", "1433").parse()?,
        database: env_or("EF_ORM_TEST_DB_NAME", "master"),
        user: env_or("EF_ORM_TEST_DB_USER", "sa"),
        password,
        trust_cert: env_or("EF_ORM_TEST_DB_TRUST_CERT", "true") != "false",
    };
    println!("connecting to {}…", config.addr());

    let ctx = DbContext::new(Pool::new(TdsDriver::new(config), PoolOptions::default()));

    ctx.execute(CREATE_TABLE, &[]).await?;
    ctx.execute("DELETE FROM [dbo].[ef_orm_tracked]", &[])
        .await?;

    // ── staged inserts: nothing written until save_changes ────────────────
    let ticket_a = ctx.dbset::<Note>().insert(Note {
        id: 0,
        text: "alpha".into(),
        version: 1,
    });
    let ticket_b = ctx.dbset::<Note>().insert(Note {
        id: 0,
        text: "beta".into(),
        version: 1,
    });
    let report = ctx.save_changes().await?;
    println!(
        "inserted={} updated={} deleted={}",
        report.inserted, report.updated, report.deleted
    );
    assert_eq!(report.inserted, 2);

    let a_id = ticket_a.resolve()?.id;
    let b_id = ticket_b.resolve()?.id;
    println!("new identity keys: a={a_id} b={b_id}");
    assert!(a_id > 0 && b_id > 0);

    // ── find snapshots the row; update stages only the diff ───────────────
    let mut a = ctx.dbset::<Note>().find(a_id).await?;
    a.text = "alpha v2".into();
    ctx.dbset::<Note>().update(&a)?;
    let report = ctx.save_changes().await?;
    println!("diff update → updated={}", report.updated);
    assert_eq!(report.updated, 1);
    assert_eq!(ctx.dbset::<Note>().find(a_id).await?.text, "alpha v2");

    // ── as_no_tracking: read-only rows, no concurrency snapshot ───────────
    let untracked = ctx.dbset::<Note>().select().as_no_tracking().all().await?;
    println!(
        "no-tracking read: {} row(s) (later updates fall back to key-only)",
        untracked.len()
    );
    assert_eq!(untracked.len(), 2);

    // ── optimistic conflict: a concurrent writer wins, the transaction rolls
    //    back, and the changes stay queued for a retry ─────────────────────
    let mut a = ctx.dbset::<Note>().find(a_id).await?;
    let mut b = ctx.dbset::<Note>().find(b_id).await?;
    a.text = "alpha v3".into();
    b.text = "beta v2".into();
    ctx.dbset::<Note>().update(&a)?;
    ctx.dbset::<Note>().update(&b)?;

    // Someone else commits first (raw SQL, outside the tracker).
    ctx.execute(
        "UPDATE [dbo].[ef_orm_tracked] SET [text] = N'intruder' WHERE [id] = @P1",
        &[Value::Int(b_id.into())],
    )
    .await?;

    match ctx.save_changes().await {
        Err(OrmError::Conflict(detail)) => println!("conflict → {detail}"),
        other => return Err(format!("expected a conflict, got {other:?}").into()),
    }

    // Rollback proof: a's queued edit was undone, b keeps the intruder's value.
    assert_eq!(
        ctx.dbset::<Note>().find(a_id).await?.text,
        "alpha v2",
        "a's update rolled back with the transaction"
    );
    assert_eq!(
        ctx.dbset::<Note>().find(b_id).await?.text,
        "intruder",
        "the concurrent write survived"
    );
    println!("rollback verified: a='alpha v2', b='intruder'");

    // ── retry: re-read (fresh snapshots) and re-stage; both commit ────────
    let mut a = ctx.dbset::<Note>().find(a_id).await?;
    a.text = format!("{} (retry)", a.text);
    ctx.dbset::<Note>().update(&a)?;
    let mut b = ctx.dbset::<Note>().find(b_id).await?;
    b.text = format!("{} (retry)", b.text);
    ctx.dbset::<Note>().update(&b)?;

    let report = ctx.save_changes().await?;
    println!("retry → updated={}", report.updated);
    assert_eq!(report.updated, 2);

    // ── deletes flush in the same unit of work ────────────────────────────
    ctx.dbset::<Note>().delete(a_id)?;
    ctx.dbset::<Note>().delete(b_id)?;
    let report = ctx.save_changes().await?;
    println!("deleted={}", report.deleted);
    assert_eq!(report.deleted, 2);

    match ctx.dbset::<Note>().find(a_id).await {
        Err(OrmError::NotFound(_)) => println!("find after delete → NotFound"),
        other => return Err(format!("expected NotFound, got {other:?}").into()),
    }

    println!("change-tracking: all steps OK");
    Ok(())
}
