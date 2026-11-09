//! Phase-5 proof: migrations against a real SQL Server.
//!
//! - `Migration` + `Schema` build DDL from the AST (never raw strings)
//! - `Migrator::up` applies pending migrations in one transaction each and
//!   records them in `__ef_migrations`; `status` classifies them
//! - introspection reads the live schema back from `INFORMATION_SCHEMA`
//! - `diff` converges a target schema; `scaffold` emits Rust entities
//! - `down` rolls migrations back (and removes the history rows)
//!
//! Credentials never live in the code: they come from environment variables
//! (same names as the other examples).
//!
//! Run with: `cargo run -p ef-orm-examples --example migrations`

use ef_orm::{Conn, Pool, PoolOptions};
use ef_orm_driver_tds::{Config, TdsDriver};
use ef_orm_migrate::diff::diff;
use ef_orm_migrate::introspect::{self, LiveColumn, LiveSchema};
use ef_orm_migrate::scaffold::scaffold;
use ef_orm_migrate::{Migration, Migrator, Schema};
use ef_orm_query::{ColumnDef, CreateIndex, CreateTable, Dialect, MssqlDialect, SqlType};
use std::env;

const TABLE: &str = "ef_orm_mig_demo";

/// Reset any leftovers from a previous run (the demo owns versions 001/002).
const CLEANUP: &str = "
IF OBJECT_ID(N'dbo.ef_orm_mig_demo', N'U') IS NOT NULL DROP TABLE [dbo].[ef_orm_mig_demo];
IF OBJECT_ID(N'dbo.__ef_migrations', N'U') IS NOT NULL
    DELETE FROM [dbo].[__ef_migrations] WHERE [version] IN (N'001', N'002');";

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn up_v1(schema: &mut Schema) {
    schema.create_table(
        CreateTable::new(TABLE)
            .column(ColumnDef::new("id", SqlType::Int).identity().primary_key())
            .column(ColumnDef::new("name", SqlType::NVarChar(100)).not_null()),
    );
}

fn down_v1(schema: &mut Schema) {
    schema.drop_table(TABLE);
}

fn up_v2(schema: &mut Schema) {
    schema.add_column(
        TABLE,
        ColumnDef::new("created_at", SqlType::DateTime2).not_null(),
    );
    schema.create_index(CreateIndex::new("ix_ef_orm_mig_demo_name", TABLE).column("name"));
}

fn down_v2(schema: &mut Schema) {
    schema.drop_index("ix_ef_orm_mig_demo_name", TABLE);
    schema.drop_column(TABLE, "created_at");
}

const V1: Migration = Migration::new("001", "create_demo", up_v1, down_v1);
const V2: Migration = Migration::new("002", "add_created_at", up_v2, down_v2);
const MIGRATIONS: [Migration; 2] = [V1, V2];

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
    let mut conn = pool.checkout().await?;
    conn.execute(CLEANUP, &[]).await?;

    // ── up: both migrations apply, each in its own transaction ────────────
    let applied = {
        let mut migrator = Migrator::new(&mut *conn);
        migrator.up(&MIGRATIONS).await?
    };
    println!("applied: {applied:?}");
    assert_eq!(applied, vec!["001".to_string(), "002".to_string()]);

    // ── up is idempotent: nothing pending ─────────────────────────────────
    let again = {
        let mut migrator = Migrator::new(&mut *conn);
        migrator.up(&MIGRATIONS).await?
    };
    assert!(again.is_empty(), "nothing should apply a second time");
    println!("second up: {} pending", again.len());

    // ── status classifies every declared migration ─────────────────────────
    let statuses = {
        let mut migrator = Migrator::new(&mut *conn);
        migrator.status(&MIGRATIONS).await?
    };
    for status in &statuses {
        println!(
            "status: {:>7} {} {}",
            if status.applied { "applied" } else { "pending" },
            status.version,
            status.name
        );
    }
    assert!(statuses.iter().all(|status| status.applied));

    // ── introspect: the live table has the columns the migrations created ──
    let live = introspect::read(&mut *conn, "dbo").await?;
    let table = live.table(TABLE).expect("the migrated table exists");
    let columns: Vec<&str> = table
        .columns
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    println!("live columns: {columns:?}");
    assert_eq!(columns, vec!["id", "name", "created_at"]);
    assert!(table.column("id").expect("id").identity);

    // ── diff: a target with one extra column converges in one statement ────
    // Diff the demo table only: the `master` database carries other examples'
    // tables, and a converging diff would (correctly) drop them.
    let mut target = table.clone();
    target.columns.push(LiveColumn {
        name: "external_ref".into(),
        ty: SqlType::NVarChar(64),
        nullable: true,
        primary_key: false,
        identity: false,
    });
    let target = LiveSchema {
        tables: vec![target],
    };
    let actual = LiveSchema {
        tables: vec![table.clone()],
    };
    let statements = diff(&target, &actual);
    println!(
        "diff: {} statement(s) → {}",
        statements.len(),
        MssqlDialect::new().build(&statements[0])?.0
    );
    assert_eq!(statements.len(), 1, "only the missing column differs");

    // ── scaffold: Rust entities from the live schema ───────────────────────
    let demo = LiveSchema {
        tables: vec![table.clone()],
    };
    let source = scaffold(&demo);
    println!("scaffold:\n{source}");
    assert!(source.contains("#[orm(primary_key, auto)]"));
    assert!(source.contains("pub struct EfOrmMigDemo"));

    // ── down: roll back the last migration, then the first ────────────────
    let rolled = {
        let mut migrator = Migrator::new(&mut *conn);
        migrator.down(&MIGRATIONS, 1).await?
    };
    println!("rolled back: {rolled:?}");
    assert_eq!(rolled, vec!["002".to_string()]);

    let rolled = {
        let mut migrator = Migrator::new(&mut *conn);
        migrator.down(&MIGRATIONS, 1).await?
    };
    assert_eq!(rolled, vec!["001".to_string()]);

    let statuses = {
        let mut migrator = Migrator::new(&mut *conn);
        migrator.status(&MIGRATIONS).await?
    };
    assert!(
        statuses.iter().all(|status| !status.applied),
        "all rolled back"
    );

    let live = introspect::read(&mut *conn, "dbo").await?;
    assert!(live.table(TABLE).is_none(), "down dropped the table");
    println!("migrations: all steps OK");
    Ok(())
}
