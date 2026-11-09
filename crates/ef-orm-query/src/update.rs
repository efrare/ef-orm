//! `UPDATE` statement AST.

use ef_orm_core::Value;

use crate::expr::{Expr, combine_filter};
use crate::ident::{Ident, Table};

/// An `UPDATE` statement with optional `OUTPUT` and `WHERE`.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateStatement {
    pub table: Table,
    pub assignments: Vec<(Ident, Value)>,
    pub filter: Option<Expr>,
    pub output: Vec<Ident>,
}

impl UpdateStatement {
    pub fn new(table: impl Into<Table>) -> Self {
        Self {
            table: table.into(),
            assignments: Vec::new(),
            filter: None,
            output: Vec::new(),
        }
    }

    /// Adds a `SET col = <value>` assignment.
    pub fn set(mut self, column: impl Into<Ident>, value: impl Into<Value>) -> Self {
        self.assignments.push((column.into(), value.into()));
        self
    }

    /// Adds a `WHERE` predicate; consecutive calls are joined with `AND`.
    pub fn and_where(mut self, filter: impl Into<Expr>) -> Self {
        combine_filter(&mut self.filter, filter.into());
        self
    }

    /// Adds an `OUTPUT INSERTED.<col>` column.
    pub fn output(mut self, name: impl Into<Ident>) -> Self {
        self.output.push(name.into());
        self
    }
}
