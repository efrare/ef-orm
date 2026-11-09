//! Phase-4 proof: typed CRUD + change tracking against a real SQL Server.
//!
//! Every write is **staged** and flushed by `save_changes` inside one
//! transaction; identity keys come back into the parked model after the
//! commit. Covers uuid/chrono/decimal mapping end-to-end
//! (insert → find → update → select → delete → NotFound).
//!
//! Credentials never live in the code: they come from environment variables.
//!
//! ```text
//! EF_ORM_TEST_DB_HOST        default 192.168.0.10
//! EF_ORM_TEST_DB_PORT        default 1433
//! EF_ORM_TEST_DB_NAME        default master
//! EF_ORM_TEST_DB_USER        default sa
//! EF_ORM_TEST_DB_PASSWORD    required (no default on purpose)
//! EF_ORM_TEST_DB_TRUST_CERT  default true (dev server, self-signed cert)
//! ```
//!
//! Run with: `cargo run -p ef-orm-examples --example hello-crud`

use ef_orm::{DbContext, Entity, Model, OrderDir, OrmError, Pool, PoolOptions};
use ef_orm_driver_tds::{Config, TdsDriver};
use std::env;

/// Demo table: id is `IDENTITY` (filled back on insert), plus one column of
/// each optional mapping type. Created idempotently below — no migration
/// engine exists before phase 6.
const CREATE_TABLE: &str = "
IF OBJECT_ID(N'dbo.ef_orm_hello', N'U') IS NULL
    CREATE TABLE [dbo].[ef_orm_hello] (
        [id] INT IDENTITY(1,1) NOT NULL PRIMARY KEY,
        [trace_id] UNIQUEIDENTIFIER NOT NULL,
        [created_at] DATETIME2 NOT NULL,
        [amount] DECIMAL(18,2) NOT NULL,
        [note] NVARCHAR(200) NULL
    );";

#[derive(Entity, Model, Debug)]
#[orm(table = "ef_orm_hello")]
struct Hello {
    #[orm(primary_key, auto)]
    id: i32,
    trace_id: uuid::Uuid,
    created_at: chrono::NaiveDateTime,
    amount: rust_decimal::Decimal,
    note: Option<String>,
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

    let pool = Pool::new(TdsDriver::new(config), PoolOptions::default());
    let ctx = DbContext::new(pool);

    // Idempotent setup + a clean slate so the example is repeatable.
    ctx.execute(CREATE_TABLE, &[]).await?;
    ctx.execute("DELETE FROM [dbo].[ef_orm_hello]", &[]).await?;

    // ── insert: staged, then flushed; identity key written back post-commit ─
    let hello = Hello {
        id: 0,
        trace_id: uuid::Uuid::new_v4(),
        created_at: chrono::Local::now().naive_local(),
        amount: rust_decimal::Decimal::new(12_345, 2), // 123.45
        note: Some("hello-crud row".into()),
    };
    let ticket = ctx.dbset::<Hello>().insert(hello);
    let report = ctx.save_changes().await?;
    println!("save_changes: inserted={}", report.inserted);
    assert_eq!(report.inserted, 1);

    let mut hello = ticket.resolve()?;
    let id = hello.id();
    println!(
        "inserted #{id} trace={} amount={}",
        hello.trace_id, hello.amount
    );
    assert!(id > 0, "the identity key was written back");

    // ── find: uuid, datetime2 and decimal come back typed (and tracked) ────
    let found = ctx.dbset::<Hello>().find(id).await?;
    println!(
        "found   #{id} created={} amount={} note={:?}",
        found.created_at, found.amount, found.note
    );
    assert_eq!(found.trace_id, hello.trace_id, "uuid round-trip");
    assert_eq!(found.amount, hello.amount, "decimal round-trip");

    // ── update: diff against the tracked originals, flushed on save_changes ─
    hello.amount = rust_decimal::Decimal::new(99_999, 2); // 999.99
    hello.note = Some("hello-crud row (edited)".into());
    ctx.dbset::<Hello>().update(&hello)?;
    let report = ctx.save_changes().await?;
    println!(
        "save_changes: updated={} → amount={}",
        report.updated, hello.amount
    );
    assert_eq!(report.updated, 1);

    // ── select: typed columns, chained filters, ordered ───────────────────
    let rows = ctx
        .dbset::<Hello>()
        .select()
        .filter(|c| c.amount.ge(rust_decimal::Decimal::new(50_000, 2)))
        .filter(|c| c.note.like("hello-crud%"))
        .order_by(Hello::columns().id, OrderDir::Asc)
        .all()
        .await?;
    println!("selected {} row(s)", rows.len());
    assert_eq!(rows.len(), 1, "only the edited row matches both filters");
    assert_eq!(rows[0].amount, rust_decimal::Decimal::new(99_999, 2));

    // ── delete + NotFound on the next find ────────────────────────────────
    ctx.dbset::<Hello>().delete(id)?;
    let report = ctx.save_changes().await?;
    println!("save_changes: deleted={}", report.deleted);
    assert_eq!(report.deleted, 1);

    match ctx.dbset::<Hello>().find(id).await {
        Err(OrmError::NotFound(detail)) => println!("find after delete → NotFound: {detail}"),
        other => return Err(format!("expected NotFound, got {other:?}").into()),
    }

    println!("hello-crud: all steps OK");
    Ok(())
}
