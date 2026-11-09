//! `INSERT` statement AST.

use ef_orm_core::Value;

use crate::ident::{Ident, Table};

/// An `INSERT` statement (multi-row, with optional `OUTPUT`).
///
/// `output` columns are rendered as `OUTPUT INSERTED.<col>` — T-SQL's
/// equivalent of `RETURNING` (used to fetch identity values).
#[derive(Debug, Clone, PartialEq)]
pub struct InsertStatement {
    pub table: Table,
    pub columns: Vec<Ident>,
    pub rows: Vec<Vec<Value>>,
    pub output: Vec<Ident>,
}

impl InsertStatement {
    pub fn new(table: impl Into<Table>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
            rows: Vec::new(),
            output: Vec::new(),
        }
    }

    pub fn column(mut self, name: impl Into<Ident>) -> Self {
        self.columns.push(name.into());
        self
    }

    pub fn columns<I>(mut self, names: I) -> Self
    where
        I: IntoIterator,
        I::Item: Into<Ident>,
    {
        self.columns.extend(names.into_iter().map(Into::into));
        self
    }

    /// Appends one row of values (call repeatedly for multi-row inserts).
    pub fn values<I, V>(mut self, row: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Value>,
    {
        self.rows.push(row.into_iter().map(Into::into).collect());
        self
    }

    /// Adds an `OUTPUT INSERTED.<col>` column.
    pub fn output(mut self, name: impl Into<Ident>) -> Self {
        self.output.push(name.into());
        self
    }
}
