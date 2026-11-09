//! `ef-orm-cli` — migrations and scaffolding for ef-ORM.
//!
//! ```text
//! ef-orm-cli --password … migrate up     [--dir migrations]
//! ef-orm-cli --password … migrate down   [--dir migrations] [--steps 1]
//! ef-orm-cli --password … migrate status [--dir migrations]
//! ef-orm-cli --password … scaffold       [--out src/entities.rs]
//! ```
//!
//! Connection settings come from flags or `EF_ORM_DB_*` environment variables
//! (the password is never logged).
//!
//! Migrations are `*.up.sql` / `*.down.sql` pairs named `<version>_<name>`:
//! `001_init.up.sql` + `001_init.down.sql`. Each pair runs inside one
//! transaction and is recorded in `__ef_migrations`.

use clap::{Args, Parser, Subcommand};
use ef_orm::Pool;
use ef_orm_driver_tds::{Config, TdsDriver};
use ef_orm_migrate::{Migrator, SqlMigration, introspect, scaffold};
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "ef-orm-cli",
    version,
    about = "Migrations and scaffolding for ef-ORM"
)]
struct Cli {
    #[command(flatten)]
    db: DbArgs,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct DbArgs {
    #[arg(long, env = "EF_ORM_DB_HOST", default_value = "localhost")]
    host: String,
    #[arg(long, env = "EF_ORM_DB_PORT", default_value_t = 1433)]
    port: u16,
    #[arg(long, env = "EF_ORM_DB_NAME", default_value = "master")]
    database: String,
    #[arg(long, env = "EF_ORM_DB_USER", default_value = "sa")]
    user: String,
    #[arg(long, env = "EF_ORM_DB_PASSWORD", hide_env_values = true)]
    password: String,
    #[arg(
        long,
        env = "EF_ORM_DB_TRUST_CERT",
        default_value_t = true,
        action = clap::ArgAction::Set
    )]
    trust_cert: bool,
    /// Schema inspected by `scaffold`.
    #[arg(long, default_value = "dbo")]
    schema: String,
}

impl DbArgs {
    fn config(&self) -> Config {
        Config {
            host: self.host.clone(),
            port: self.port,
            database: self.database.clone(),
            user: self.user.clone(),
            password: self.password.clone(),
            trust_cert: self.trust_cert,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Apply, roll back or inspect SQL-file migrations.
    Migrate {
        #[command(subcommand)]
        action: Migrate,
    },
    /// Generate `#[derive(Entity, Model)]` structs from the live schema.
    Scaffold {
        /// Write the generated source here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum Migrate {
    /// Apply every pending `*.up.sql` migration.
    Up {
        #[arg(long, default_value = "migrations")]
        dir: PathBuf,
    },
    /// Roll back the last `--steps` applied migrations.
    Down {
        #[arg(long, default_value = "migrations")]
        dir: PathBuf,
        #[arg(long, default_value_t = 1)]
        steps: usize,
    },
    /// List declared migrations and whether they are applied.
    Status {
        #[arg(long, default_value = "migrations")]
        dir: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let pool = Pool::new(TdsDriver::new(cli.db.config()), Default::default());
    let mut conn = pool.checkout().await?;

    match cli.command {
        Command::Migrate { action } => {
            let mut migrator = Migrator::new(&mut *conn);
            match action {
                Migrate::Up { dir } => {
                    let ran = migrator.up_sql(&load_migrations(&dir)?).await?;
                    report(&ran, "applied", "database is up to date");
                }
                Migrate::Down { dir, steps } => {
                    let rolled = migrator.down_sql(&load_migrations(&dir)?, steps).await?;
                    report(&rolled, "rolled back", "no applied migrations");
                }
                Migrate::Status { dir } => {
                    let statuses = migrator.status_sql(&load_migrations(&dir)?).await?;
                    for status in statuses {
                        let state = if status.applied { "applied" } else { "pending" };
                        println!("{state:>7}  {}  {}", status.version, status.name);
                    }
                }
            }
        }
        Command::Scaffold { out } => {
            let live = introspect::read(&mut *conn, &cli.db.schema).await?;
            let source = scaffold::scaffold(&live);
            match out {
                Some(path) => {
                    std::fs::write(&path, source)?;
                    println!("wrote {}", path.display());
                }
                None => print!("{source}"),
            }
        }
    }
    Ok(())
}

fn report(versions: &[String], verb: &str, empty: &str) {
    if versions.is_empty() {
        println!("{empty}");
        return;
    }
    for version in versions {
        println!("{verb} {version}");
    }
}

/// Loads `<version>_<name>.up.sql` (+ optional `.down.sql`) pairs, sorted.
fn load_migrations(dir: &Path) -> Result<Vec<SqlMigration>, Box<dyn Error>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    entries.sort();

    let mut migrations = Vec::new();
    for path in entries {
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        let Some(stem) = file.strip_suffix(".up.sql") else {
            continue;
        };
        let (version, name) = split_stem(stem);
        let down_sql =
            read_sql(&path.with_file_name(format!("{stem}.down.sql"))).unwrap_or_default();
        migrations.push(SqlMigration {
            version: version.to_string(),
            name: name.to_string(),
            up_sql: read_sql(&path)?,
            down_sql,
        });
    }

    if migrations.is_empty() {
        return Err(format!("no '*.up.sql' files found in {}", dir.display()).into());
    }
    Ok(migrations)
}

/// `001_init` → (`001`, `init`); without a separator the stem is both.
fn split_stem(stem: &str) -> (&str, &str) {
    stem.split_once('_').unwrap_or((stem, stem))
}

/// Reads a SQL file, dropping a UTF-8 BOM (Windows editors add one).
fn read_sql(path: &Path) -> Result<String, Box<dyn Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(match text.strip_prefix('\u{feff}') {
        Some(text) => text.to_string(),
        None => text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_version_and_name_from_file_stems() {
        assert_eq!(split_stem("001_init"), ("001", "init"));
        assert_eq!(
            split_stem("20261109_create_users"),
            ("20261109", "create_users")
        );
        assert_eq!(split_stem("init"), ("init", "init"));
    }
}
