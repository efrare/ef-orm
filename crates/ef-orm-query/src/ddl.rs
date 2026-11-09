//! DDL statement AST: tables, columns, indexes and constraints.
//!
//! Migrations build these nodes and the [`Dialect`](crate::Dialect) renders
//! them â€” DDL never travels as loose strings. Types are MSSQL-first; a new
//! dialect maps [`SqlType`] to its own names in the renderer.

use crate::ident::{Ident, Table};
use ef_orm_core::Value;

mod constraint;

pub use constraint::{AddForeignKey, DropConstraint, RefAction};

/// Column type in the engine's catalog (T-SQL names live in the renderer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SqlType {
    Int,
    BigInt,
    SmallInt,
    TinyInt,
    Bit,
    Float,
    Real,
    Date,
    DateTime,
    DateTime2,
    DateTimeOffset,
    Time,
    UniqueIdentifier,
    Decimal {
        precision: u8,
        scale: u8,
    },
    NVarChar(u32),
    /// `NVARCHAR(MAX)`.
    NVarCharMax,
    VarChar(u32),
    /// `VARCHAR(MAX)`.
    VarCharMax,
    Char(u32),
    NChar(u32),
    VarBinary(u32),
    /// `VARBINARY(MAX)`.
    VarBinaryMax,
    Binary(u32),
}

/// Value assigned to a column that is omitted from an `INSERT`.
#[derive(Debug, Clone, PartialEq)]
pub enum DefaultValue {
    /// Rendered inline as a T-SQL literal (`N'x'`, `42`, `0x0A`).
    Literal(Value),
    /// `SYSUTCDATETIME()` â€” the history table's `applied_at`.
    SysUtcDateTime,
}

/// One column definition (`CREATE TABLE` / `ALTER TABLE`).
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: Ident,
    pub ty: SqlType,
    pub nullable: bool,
    pub identity: bool,
    pub primary_key: bool,
    pub default: Option<DefaultValue>,
}

impl ColumnDef {
    /// A nullable column with no default.
    pub fn new(name: impl Into<Ident>, ty: SqlType) -> Self {
        Self {
            name: name.into(),
            ty,
            nullable: true,
            identity: false,
            primary_key: false,
            default: None,
        }
    }

    /// Makes the column `NOT NULL`.
    pub fn not_null(mut self) -> Self {
        self.nullable = false;
        self
    }

    /// Marks the column `IDENTITY(1,1)` (implies `NOT NULL`).
    pub fn identity(mut self) -> Self {
        self.identity = true;
        self.nullable = false;
        self
    }

    /// Marks the column as the table's `PRIMARY KEY` (implies `NOT NULL`).
    pub fn primary_key(mut self) -> Self {
        self.primary_key = true;
        self.nullable = false;
        self
    }

    /// Sets the `DEFAULT` clause.
    pub fn default(mut self, value: DefaultValue) -> Self {
        self.default = Some(value);
        self
    }
}

/// `CREATE TABLE â€¦`.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateTable {
    pub table: Table,
    pub columns: Vec<ColumnDef>,
}

impl CreateTable {
    pub fn new(table: impl Into<Table>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
        }
    }

    /// Appends a column (declaration order is preserved).
    pub fn column(mut self, column: ColumnDef) -> Self {
        self.columns.push(column);
        self
    }
}

/// `DROP TABLE â€¦`.
#[derive(Debug, Clone, PartialEq)]
pub struct DropTable {
    pub table: Table,
}

impl DropTable {
    pub fn new(table: impl Into<Table>) -> Self {
        Self {
            table: table.into(),
        }
    }
}

/// `ALTER TABLE â€¦ ADD <column>`.
#[derive(Debug, Clone, PartialEq)]
pub struct AddColumn {
    pub table: Table,
    pub column: ColumnDef,
}

impl AddColumn {
    pub fn new(table: impl Into<Table>, column: ColumnDef) -> Self {
        Self {
            table: table.into(),
            column,
        }
    }
}

/// `ALTER TABLE â€¦ DROP COLUMN <column>`.
#[derive(Debug, Clone, PartialEq)]
pub struct DropColumn {
    pub table: Table,
    pub column: Ident,
}

impl DropColumn {
    pub fn new(table: impl Into<Table>, column: impl Into<Ident>) -> Self {
        Self {
            table: table.into(),
            column: column.into(),
        }
    }
}

/// `ALTER TABLE â€¦ ALTER COLUMN <column>` (type/nulls change; not identity).
#[derive(Debug, Clone, PartialEq)]
pub struct AlterColumn {
    pub table: Table,
    pub column: ColumnDef,
}

impl AlterColumn {
    pub fn new(table: impl Into<Table>, column: ColumnDef) -> Self {
        Self {
            table: table.into(),
            column,
        }
    }
}

/// `CREATE [UNIQUE] INDEX â€¦`.
#[derive(Debug, Clone, PartialEq)]
pub struct CreateIndex {
    pub name: Ident,
    pub table: Table,
    pub columns: Vec<Ident>,
    pub unique: bool,
}

impl CreateIndex {
    pub fn new(name: impl Into<Ident>, table: impl Into<Table>) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
            columns: Vec::new(),
            unique: false,
        }
    }

    pub fn column(mut self, column: impl Into<Ident>) -> Self {
        self.columns.push(column.into());
        self
    }

    /// Emits `CREATE UNIQUE INDEX` instead of `CREATE INDEX`.
    pub fn unique(mut self) -> Self {
        self.unique = true;
        self
    }
}

/// `DROP INDEX â€¦ ON <table>`.
#[derive(Debug, Clone, PartialEq)]
pub struct DropIndex {
    pub name: Ident,
    pub table: Table,
}

impl DropIndex {
    pub fn new(name: impl Into<Ident>, table: impl Into<Table>) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
        }
    }
}
