//! T-SQL rendering of DDL (tables, columns, indexes, constraints).
//!
//! Every node renders to a single statement with **no bound parameters**:
//! DDL literals (defaults) are inlined with T-SQL escaping.

use ef_orm_core::Value;

use crate::ddl::{
    AddColumn, AddForeignKey, AlterColumn, ColumnDef, CreateIndex, CreateTable, DefaultValue,
    DropColumn, DropConstraint, DropIndex, DropTable, SqlType,
};
use crate::error::{QueryError, QueryResult};
use crate::ident::Ident;
use crate::mssql::MssqlDialect;
use crate::renderer::Renderer;

pub(crate) fn render_create_table(
    dialect: &MssqlDialect,
    stmt: &CreateTable,
) -> QueryResult<(String, Vec<Value>)> {
    if stmt.columns.is_empty() {
        return Err(QueryError::invalid("CREATE TABLE without columns"));
    }

    let mut r = Renderer::new(dialect);
    r.push("CREATE TABLE ");
    r.table(&stmt.table);
    r.push(" (");
    for (i, column) in stmt.columns.iter().enumerate() {
        if i > 0 {
            r.push(", ");
        }
        render_column_def(&mut r, column)?;
    }
    r.push(")");
    Ok(r.finish())
}

pub(crate) fn render_drop_table(
    dialect: &MssqlDialect,
    stmt: &DropTable,
) -> QueryResult<(String, Vec<Value>)> {
    let mut r = Renderer::new(dialect);
    r.push("DROP TABLE ");
    r.table(&stmt.table);
    Ok(r.finish())
}

pub(crate) fn render_add_column(
    dialect: &MssqlDialect,
    stmt: &AddColumn,
) -> QueryResult<(String, Vec<Value>)> {
    let mut r = Renderer::new(dialect);
    r.push("ALTER TABLE ");
    r.table(&stmt.table);
    r.push(" ADD ");
    render_column_def(&mut r, &stmt.column)?;
    Ok(r.finish())
}

pub(crate) fn render_drop_column(
    dialect: &MssqlDialect,
    stmt: &DropColumn,
) -> QueryResult<(String, Vec<Value>)> {
    let mut r = Renderer::new(dialect);
    r.push("ALTER TABLE ");
    r.table(&stmt.table);
    r.push(" DROP COLUMN ");
    r.ident(&stmt.column);
    Ok(r.finish())
}

pub(crate) fn render_alter_column(
    dialect: &MssqlDialect,
    stmt: &AlterColumn,
) -> QueryResult<(String, Vec<Value>)> {
    // `ALTER COLUMN` supports type + nullability only (no IDENTITY/DEFAULT).
    let mut r = Renderer::new(dialect);
    r.push("ALTER TABLE ");
    r.table(&stmt.table);
    r.push(" ALTER COLUMN ");
    r.ident(&stmt.column.name);
    r.push(" ");
    r.push(&sql_type(&stmt.column.ty));
    r.push(if stmt.column.nullable {
        " NULL"
    } else {
        " NOT NULL"
    });
    Ok(r.finish())
}

pub(crate) fn render_create_index(
    dialect: &MssqlDialect,
    stmt: &CreateIndex,
) -> QueryResult<(String, Vec<Value>)> {
    if stmt.columns.is_empty() {
        return Err(QueryError::invalid("CREATE INDEX without columns"));
    }

    let mut r = Renderer::new(dialect);
    r.push(if stmt.unique {
        "CREATE UNIQUE INDEX "
    } else {
        "CREATE INDEX "
    });
    r.ident(&stmt.name);
    r.push(" ON ");
    r.table(&stmt.table);
    r.push(" (");
    push_idents(&mut r, &stmt.columns);
    r.push(")");
    Ok(r.finish())
}

pub(crate) fn render_drop_index(
    dialect: &MssqlDialect,
    stmt: &DropIndex,
) -> QueryResult<(String, Vec<Value>)> {
    let mut r = Renderer::new(dialect);
    r.push("DROP INDEX ");
    r.ident(&stmt.name);
    r.push(" ON ");
    r.table(&stmt.table);
    Ok(r.finish())
}

pub(crate) fn render_add_foreign_key(
    dialect: &MssqlDialect,
    stmt: &AddForeignKey,
) -> QueryResult<(String, Vec<Value>)> {
    if stmt.columns.is_empty() {
        return Err(QueryError::invalid("FOREIGN KEY without columns"));
    }
    if stmt.columns.len() != stmt.ref_columns.len() {
        return Err(QueryError::invalid(format!(
            "FOREIGN KEY has {} columns for {} referenced columns",
            stmt.columns.len(),
            stmt.ref_columns.len()
        )));
    }

    let mut r = Renderer::new(dialect);
    r.push("ALTER TABLE ");
    r.table(&stmt.table);
    r.push(" ADD CONSTRAINT ");
    r.ident(&stmt.name);
    r.push(" FOREIGN KEY (");
    push_idents(&mut r, &stmt.columns);
    r.push(") REFERENCES ");
    r.table(&stmt.ref_table);
    r.push(" (");
    push_idents(&mut r, &stmt.ref_columns);
    r.push(")");
    if let Some(action) = stmt.on_delete {
        r.push(" ON DELETE ");
        r.push(action.as_sql());
    }
    if let Some(action) = stmt.on_update {
        r.push(" ON UPDATE ");
        r.push(action.as_sql());
    }
    Ok(r.finish())
}

pub(crate) fn render_drop_constraint(
    dialect: &MssqlDialect,
    stmt: &DropConstraint,
) -> QueryResult<(String, Vec<Value>)> {
    let mut r = Renderer::new(dialect);
    r.push("ALTER TABLE ");
    r.table(&stmt.table);
    r.push(" DROP CONSTRAINT ");
    r.ident(&stmt.name);
    Ok(r.finish())
}

fn render_column_def(r: &mut Renderer<'_>, column: &ColumnDef) -> QueryResult<()> {
    r.ident(&column.name);
    r.push(" ");
    r.push(&sql_type(&column.ty));
    if column.identity {
        r.push(" IDENTITY(1,1)");
    }
    r.push(if column.nullable {
        " NULL"
    } else {
        " NOT NULL"
    });
    if let Some(default) = &column.default {
        r.push(" DEFAULT ");
        match default {
            DefaultValue::Literal(value) => r.push(&literal(value)?),
            DefaultValue::SysUtcDateTime => r.push("SYSUTCDATETIME()"),
        }
    }
    if column.primary_key {
        r.push(" PRIMARY KEY");
    }
    Ok(())
}

/// Renders a literal suitable for DDL (`DEFAULT`): strings are Unicode and
/// single quotes doubled; bytes are hex; unsupported shapes are rejected.
fn literal(value: &Value) -> QueryResult<String> {
    match value {
        Value::Null => Ok("NULL".into()),
        Value::Bool(flag) => Ok(if *flag { "1".into() } else { "0".into() }),
        Value::Int(number) => Ok(number.to_string()),
        Value::Float(number) => Ok(number.to_string()),
        Value::Str(text) => Ok(format!("N'{}'", text.replace('\'', "''"))),
        Value::Bytes(bytes) => {
            let mut hex = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                hex.push_str(&format!("{byte:02X}"));
            }
            Ok(format!("0x{hex}"))
        }
        // Feature-gated `Value` variants (uuid/chrono/decimal) only exist in
        // builds that enable them; in a bare build this arm is unreachable.
        #[allow(unreachable_patterns)]
        other => Err(QueryError::invalid(format!(
            "unsupported DEFAULT literal: {other:?}"
        ))),
    }
}

fn sql_type(ty: &SqlType) -> String {
    match ty {
        SqlType::Int => "INT".into(),
        SqlType::BigInt => "BIGINT".into(),
        SqlType::SmallInt => "SMALLINT".into(),
        SqlType::TinyInt => "TINYINT".into(),
        SqlType::Bit => "BIT".into(),
        SqlType::Float => "FLOAT".into(),
        SqlType::Real => "REAL".into(),
        SqlType::Date => "DATE".into(),
        SqlType::DateTime => "DATETIME".into(),
        SqlType::DateTime2 => "DATETIME2".into(),
        SqlType::DateTimeOffset => "DATETIMEOFFSET".into(),
        SqlType::Time => "TIME".into(),
        SqlType::UniqueIdentifier => "UNIQUEIDENTIFIER".into(),
        SqlType::Decimal { precision, scale } => format!("DECIMAL({precision},{scale})"),
        SqlType::NVarChar(length) => format!("NVARCHAR({length})"),
        SqlType::NVarCharMax => "NVARCHAR(MAX)".into(),
        SqlType::VarChar(length) => format!("VARCHAR({length})"),
        SqlType::VarCharMax => "VARCHAR(MAX)".into(),
        SqlType::Char(length) => format!("CHAR({length})"),
        SqlType::NChar(length) => format!("NCHAR({length})"),
        SqlType::VarBinary(length) => format!("VARBINARY({length})"),
        SqlType::VarBinaryMax => "VARBINARY(MAX)".into(),
        SqlType::Binary(length) => format!("BINARY({length})"),
    }
}

fn push_idents(r: &mut Renderer<'_>, idents: &[Ident]) {
    for (i, ident) in idents.iter().enumerate() {
        if i > 0 {
            r.push(", ");
        }
        r.ident(ident);
    }
}
