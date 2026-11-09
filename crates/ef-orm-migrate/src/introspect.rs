//! Live-schema introspection over `INFORMATION_SCHEMA` / `sys`.
//!
//! Every query is built from the AST and rendered by [`MssqlDialect`] Ã¢â‚¬â€ the
//! engine never writes SQL by hand. Joins are avoided on purpose (the AST has
//! none yet): the small result sets are joined in Rust.

use std::collections::BTreeMap;

use ef_orm::{Conn, DriverError, OrmError, Row, RowData};
use ef_orm_query::{Dialect, Expr, MssqlDialect, OrderDir, SelectStatement, SqlType, Table};

/// A table as it exists in the database right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTable {
    pub name: String,
    pub columns: Vec<LiveColumn>,
}

impl LiveTable {
    /// Looks up a column by name (case-insensitive, like T-SQL).
    pub fn column(&self, name: &str) -> Option<&LiveColumn> {
        self.columns
            .iter()
            .find(|column| column.name.eq_ignore_ascii_case(name))
    }
}

/// A live column: type, nullability and key/identity markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveColumn {
    pub name: String,
    pub ty: SqlType,
    pub nullable: bool,
    pub primary_key: bool,
    pub identity: bool,
}

/// Every base table of one schema.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveSchema {
    pub tables: Vec<LiveTable>,
}

impl LiveSchema {
    /// Looks up a table by name (case-insensitive).
    pub fn table(&self, name: &str) -> Option<&LiveTable> {
        self.tables
            .iter()
            .find(|table| table.name.eq_ignore_ascii_case(name))
    }
}

/// Reads every base table and column of `schema` (usually `dbo`).
pub async fn read<C: Conn>(conn: &mut C, schema: &str) -> Result<LiveSchema, OrmError> {
    let table_names = fetch(conn, tables_query(schema)).await?;
    let column_rows = fetch(conn, columns_query(schema)).await?;
    let key_rows = fetch(conn, key_columns_query(schema)).await?;
    let constraint_rows = fetch(conn, primary_key_constraints_query(schema)).await?;
    let primary_keys = primary_keys(&key_rows, &constraint_rows)?;
    let identities = identities(conn).await?;

    let mut by_table: BTreeMap<String, Vec<LiveColumn>> = BTreeMap::new();
    for row in &column_rows {
        let table = required_string(row, "TABLE_NAME")?;
        let name = required_string(row, "COLUMN_NAME")?;
        let nullability = required_string(row, "IS_NULLABLE")?;
        let ty = parse_sql_type(
            &required_string(row, "DATA_TYPE")?,
            optional_int(row, "CHARACTER_MAXIMUM_LENGTH")?,
            optional_int(row, "NUMERIC_PRECISION")?,
            optional_int(row, "NUMERIC_SCALE")?,
        )?;
        let is_key = |set: &std::collections::BTreeSet<(String, String)>| {
            set.contains(&(table.to_lowercase(), name.to_lowercase()))
        };
        by_table.entry(table.clone()).or_default().push(LiveColumn {
            primary_key: is_key(&primary_keys),
            identity: is_key(&identities),
            name,
            ty,
            nullable: nullability.eq_ignore_ascii_case("YES"),
        });
    }

    let mut tables = Vec::with_capacity(table_names.len());
    for row in &table_names {
        let name = required_string(row, "TABLE_NAME")?;
        let columns = by_table.remove(&name).unwrap_or_default();
        tables.push(LiveTable { name, columns });
    }
    Ok(LiveSchema { tables })
}

async fn fetch<C: Conn>(conn: &mut C, stmt: SelectStatement) -> Result<Vec<RowData>, OrmError> {
    let (sql, params) = MssqlDialect::new()
        .build(&stmt.into())
        .map_err(|error| OrmError::Database(error.to_string()))?;
    conn.query(&sql, &params).await.map_err(db_error)
}

fn tables_query(schema: &str) -> SelectStatement {
    SelectStatement::new(Table::with_schema("INFORMATION_SCHEMA", "TABLES"))
        .columns(["TABLE_NAME"])
        .and_where(Expr::col("TABLE_TYPE").eq(Expr::val("BASE TABLE")))
        .and_where(Expr::col("TABLE_SCHEMA").eq(Expr::val(schema)))
        .order_by("TABLE_NAME", OrderDir::Asc)
}

fn columns_query(schema: &str) -> SelectStatement {
    SelectStatement::new(Table::with_schema("INFORMATION_SCHEMA", "COLUMNS"))
        .columns([
            "TABLE_NAME",
            "COLUMN_NAME",
            "DATA_TYPE",
            "IS_NULLABLE",
            "CHARACTER_MAXIMUM_LENGTH",
            "NUMERIC_PRECISION",
            "NUMERIC_SCALE",
        ])
        .and_where(Expr::col("TABLE_SCHEMA").eq(Expr::val(schema)))
        .order_by("TABLE_NAME", OrderDir::Asc)
        .order_by("ORDINAL_POSITION", OrderDir::Asc)
}

fn key_columns_query(schema: &str) -> SelectStatement {
    SelectStatement::new(Table::with_schema("INFORMATION_SCHEMA", "KEY_COLUMN_USAGE"))
        .columns(["TABLE_NAME", "COLUMN_NAME", "CONSTRAINT_NAME"])
        .and_where(Expr::col("TABLE_SCHEMA").eq(Expr::val(schema)))
}

fn primary_key_constraints_query(schema: &str) -> SelectStatement {
    SelectStatement::new(Table::with_schema(
        "INFORMATION_SCHEMA",
        "TABLE_CONSTRAINTS",
    ))
    .columns(["CONSTRAINT_NAME"])
    .and_where(Expr::col("CONSTRAINT_TYPE").eq(Expr::val("PRIMARY KEY")))
    .and_where(Expr::col("TABLE_SCHEMA").eq(Expr::val(schema)))
}

fn identity_columns_query() -> SelectStatement {
    SelectStatement::new(Table::with_schema("sys", "identity_columns"))
        .columns(["object_id", "name"])
}

fn sys_tables_query() -> SelectStatement {
    SelectStatement::new(Table::with_schema("sys", "tables")).columns(["object_id", "name"])
}

type KeySet = std::collections::BTreeSet<(String, String)>;

fn primary_keys(rows: &[RowData], constraints: &[RowData]) -> Result<KeySet, OrmError> {
    let primary: std::collections::BTreeSet<String> = constraints
        .iter()
        .map(|row| required_string(row, "CONSTRAINT_NAME"))
        .collect::<Result<_, OrmError>>()?;

    let mut keys = KeySet::new();
    for row in rows {
        let constraint = required_string(row, "CONSTRAINT_NAME")?;
        if !primary.contains(&constraint) {
            continue;
        }
        keys.insert((
            required_string(row, "TABLE_NAME")?.to_lowercase(),
            required_string(row, "COLUMN_NAME")?.to_lowercase(),
        ));
    }
    Ok(keys)
}

async fn identities<C: Conn>(conn: &mut C) -> Result<KeySet, OrmError> {
    let tables = fetch(conn, sys_tables_query()).await?;
    let mut names: BTreeMap<i64, String> = BTreeMap::new();
    for row in &tables {
        names.insert(
            required_int(row, "object_id")?,
            required_string(row, "name")?,
        );
    }

    let columns = fetch(conn, identity_columns_query()).await?;
    let mut keys = KeySet::new();
    for row in &columns {
        if let Some(table) = names.get(&required_int(row, "object_id")?) {
            keys.insert((
                table.to_lowercase(),
                required_string(row, "name")?.to_lowercase(),
            ));
        }
    }
    Ok(keys)
}

/// Maps `INFORMATION_SCHEMA` metadata to the portable [`SqlType`].
fn parse_sql_type(
    data_type: &str,
    max_length: Option<i64>,
    precision: Option<i64>,
    scale: Option<i64>,
) -> Result<SqlType, OrmError> {
    let ty = match data_type.to_lowercase().as_str() {
        "int" => SqlType::Int,
        "bigint" => SqlType::BigInt,
        "smallint" => SqlType::SmallInt,
        "tinyint" => SqlType::TinyInt,
        "bit" => SqlType::Bit,
        "float" => SqlType::Float,
        "real" => SqlType::Real,
        "date" => SqlType::Date,
        "datetime" => SqlType::DateTime,
        "datetime2" => SqlType::DateTime2,
        "datetimeoffset" => SqlType::DateTimeOffset,
        "time" => SqlType::Time,
        "uniqueidentifier" => SqlType::UniqueIdentifier,
        "decimal" | "numeric" => SqlType::Decimal {
            precision: precision.unwrap_or(18) as u8,
            scale: scale.unwrap_or(0) as u8,
        },
        "nvarchar" => length(max_length, SqlType::NVarCharMax, SqlType::NVarChar),
        "varchar" => length(max_length, SqlType::VarCharMax, SqlType::VarChar),
        "nchar" => SqlType::NChar(max_length.unwrap_or(1) as u32),
        "char" => SqlType::Char(max_length.unwrap_or(1) as u32),
        "varbinary" => length(max_length, SqlType::VarBinaryMax, SqlType::VarBinary),
        "binary" => SqlType::Binary(max_length.unwrap_or(1) as u32),
        other => {
            return Err(OrmError::Conversion(format!(
                "unsupported column type '{other}'"
            )));
        }
    };
    Ok(ty)
}

/// `-1` / absent length means `MAX`; otherwise the concrete length.
fn length(max_length: Option<i64>, max: SqlType, sized: impl Fn(u32) -> SqlType) -> SqlType {
    match max_length {
        Some(-1) | None => max,
        Some(value) => sized(value as u32),
    }
}

fn required_string(row: &impl Row, column: &str) -> Result<String, OrmError> {
    row.get_by_name::<String>(column)?
        .ok_or_else(|| OrmError::Conversion(format!("{column} is NULL")))
}

fn required_int(row: &impl Row, column: &str) -> Result<i64, OrmError> {
    row.get_by_name::<i64>(column)?
        .ok_or_else(|| OrmError::Conversion(format!("{column} is NULL")))
}

fn optional_int(row: &impl Row, column: &str) -> Result<Option<i64>, OrmError> {
    row.get_by_name::<i64>(column)
}

fn db_error(error: DriverError) -> OrmError {
    OrmError::Database(error.to_string())
}

#[cfg(test)]
mod tests;
