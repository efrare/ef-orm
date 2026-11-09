//! Phase-1 spike: dial SQL Server through the ef-ORM pool, run `SELECT 1`
//! and print the value — the roadmap's "proof of life" for the TDS stack.
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
//! Run with: `cargo run -p ef-orm-examples --example hello-raw`

use ef_orm_driver::{Conn, Pool, PoolOptions, Row};
use ef_orm_driver_tds::{Config, TdsDriver};
use std::env;

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

    println!(
        "connecting to {} (database {})…",
        config.addr(),
        config.database
    );

    // Phase 1 exercises the whole stack: pool → traits → TDS adapter.
    let pool = Pool::new(TdsDriver::new(config), PoolOptions::default());
    let mut conn = pool.checkout().await?;

    let rows = conn.query("SELECT 1 AS [value]", &[]).await?;
    let value: i32 = rows
        .first()
        .ok_or("SELECT 1 returned no rows")?
        .get(0)?
        .ok_or("SELECT 1 returned NULL")?;

    println!("SELECT 1 → {value}");
    Ok(())
}
