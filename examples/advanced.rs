//! Phase-6 proof against a real SQL Server:
//!
//! - `raw_sql` — escape hatch with bound parameters and partial `FromRow`
//! - `select().stream(&mut conn)` — rows consumed incrementally (no full
//!   materialization)
//! - `insert_many` — one `BULK INSERT` batch (Tiberius `BulkLoadRequest`)
//! - `find_in` — batched child load (two queries total, never N+1)
//! - `all_as` — partial projection into a custom row type
//!
//! Credentials never live in the code: they come from environment variables
//! (same names as the other examples).
//!
//! Run with: `cargo run -p ef-orm-examples --example advanced`

use std::collections::HashMap;

use ef_orm::{DbContext, Entity, FromRow, Model, OrmError, Pool, PoolOptions, Row};
use ef_orm_driver_tds::{Config, TdsDriver};
use futures_util::StreamExt;
use std::env;

const SETUP: &str = "
IF OBJECT_ID(N'dbo.ef_orm_posts', N'U') IS NOT NULL DROP TABLE [dbo].[ef_orm_posts];
IF OBJECT_ID(N'dbo.ef_orm_authors', N'U') IS NOT NULL DROP TABLE [dbo].[ef_orm_authors];
CREATE TABLE [dbo].[ef_orm_authors] (
    [code] NVARCHAR(20) NOT NULL PRIMARY KEY,
    [name] NVARCHAR(100) NOT NULL
);
CREATE TABLE [dbo].[ef_orm_posts] (
    [id] INT IDENTITY(1,1) NOT NULL PRIMARY KEY,
    [author_code] NVARCHAR(20) NOT NULL,
    [title] NVARCHAR(100) NOT NULL
);";

#[derive(Entity, Model, Debug)]
#[orm(table = "ef_orm_authors")]
struct Author {
    #[orm(primary_key)]
    code: String,
    name: String,
}

#[derive(Entity, Model, Debug)]
#[orm(table = "ef_orm_posts")]
struct Post {
    #[orm(primary_key, auto)]
    id: i32,
    author_code: String,
    title: String,
}

/// A narrow read: only `title` is projected (partial `FromRow`).
struct TitleOnly {
    title: String,
}

impl FromRow for TitleOnly {
    fn from_row<R: Row>(row: &R) -> Result<Self, OrmError> {
        Ok(Self {
            title: row
                .get_by_name("title")?
                .ok_or_else(|| OrmError::Conversion("title is NULL".into()))?,
        })
    }
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
    ctx.execute(SETUP, &[]).await?;

    // ── bulk insert: one BATCH for all authors, then all posts ─────────────
    let authors = [
        Author {
            code: "ada".into(),
            name: "Ada Lovelace".into(),
        },
        Author {
            code: "grace".into(),
            name: "Grace Hopper".into(),
        },
    ];
    let inserted = ctx.dbset::<Author>().insert_many(&authors).await?;
    println!("bulk authors: {inserted}");
    assert_eq!(inserted, 2);

    let posts = [
        Post {
            id: 0,
            author_code: "ada".into(),
            title: "Analytical Engine".into(),
        },
        Post {
            id: 0,
            author_code: "ada".into(),
            title: "Notes on Bernoulli".into(),
        },
        Post {
            id: 0,
            author_code: "grace".into(),
            title: "COBOL compiler".into(),
        },
    ];
    let inserted = ctx.dbset::<Post>().insert_many(&posts).await?;
    println!("bulk posts: {inserted}");
    assert_eq!(inserted, 3);

    // ── raw SQL: a scalar with a bound parameter ──────────────────────────
    let total: i64 = ctx
        .raw_sql("SELECT COUNT(*) AS [total] FROM [dbo].[ef_orm_posts] WHERE [author_code] = @P1")
        .param("ada")
        .fetch_one::<Count>()
        .await?
        .map(|count| count.total)
        .unwrap_or_default();
    println!("raw count for 'ada': {total}");
    assert_eq!(total, 2);

    // ── streaming: consume rows one at a time over a checked-out conn ─────
    let mut conn = ctx.checkout().await?;
    let mut stream = ctx.dbset::<Post>().select().stream(&mut *conn).await?;
    let mut streamed = 0;
    while let Some(post) = stream.next().await {
        let post = post?;
        assert!(!post.title.is_empty());
        streamed += 1;
    }
    drop(stream);
    println!("streamed posts: {streamed}");
    assert_eq!(streamed, 3);

    // ── eager load: parents + one batched child query (no N+1) ────────────
    let authors = ctx.dbset::<Author>().select().all().await?;
    let children = ctx
        .dbset::<Post>()
        .find_in(
            Post::columns().author_code,
            authors.iter().map(|author| author.code.clone()),
        )
        .await?;

    let mut by_author: HashMap<String, Vec<Post>> = HashMap::new();
    for post in children {
        by_author
            .entry(post.author_code.clone())
            .or_default()
            .push(post);
    }
    for author in &authors {
        let titles: Vec<&str> = by_author
            .get(&author.code)
            .map(|posts| posts.iter().map(|post| post.title.as_str()).collect())
            .unwrap_or_default();
        println!("{} → {titles:?}", author.name);
    }
    assert_eq!(by_author.get("ada").map(Vec::len), Some(2));
    assert_eq!(by_author.get("grace").map(Vec::len), Some(1));

    // ── partial projection: only `title` comes back ───────────────────────
    let titles = ctx
        .dbset::<Post>()
        .select()
        .columns(["title"])
        .all_as::<TitleOnly>()
        .await?;
    println!(
        "partial projection: {} title(s), first={:?}",
        titles.len(),
        titles.first().map(|row| row.title.as_str())
    );
    assert_eq!(titles.len(), 3);

    // ── cleanup ───────────────────────────────────────────────────────────
    ctx.execute(
        "IF OBJECT_ID(N'dbo.ef_orm_posts', N'U') IS NOT NULL DROP TABLE [dbo].[ef_orm_posts];\
         IF OBJECT_ID(N'dbo.ef_orm_authors', N'U') IS NOT NULL DROP TABLE [dbo].[ef_orm_authors];",
        &[],
    )
    .await?;

    println!("advanced: all steps OK");
    Ok(())
}

/// Row type for the raw scalar read.
struct Count {
    total: i64,
}

impl FromRow for Count {
    fn from_row<R: Row>(row: &R) -> Result<Self, OrmError> {
        Ok(Self {
            total: row
                .get_by_name("total")?
                .ok_or_else(|| OrmError::Conversion("total is NULL".into()))?,
        })
    }
}
