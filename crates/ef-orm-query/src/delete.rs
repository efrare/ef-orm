//! `DELETE` statement AST.

use crate::expr::{Expr, combine_filter};
use crate::ident::{Ident, Table};

/// A `DELETE` statement with optional `OUTPUT` and `WHERE`.
#[derive(Debug, Clone, PartialEq)]
pub struct DeleteStatement {
    pub table: Table,
    pub filter: Option<Expr>,
    pub output: Vec<Ident>,
}

impl DeleteStatement {
    pub fn new(table: impl Into<Table>) -> Self {
        Self {
            table: table.into(),
            filter: None,
            output: Vec::new(),
        }
    }

    /// Adds a `WHERE` predicate; consecutive calls are joined with `AND`.
    pub fn and_where(mut self, filter: impl Into<Expr>) -> Self {
        combine_filter(&mut self.filter, filter.into());
        self
    }

    /// Adds an `OUTPUT DELETED.<col>` column.
    pub fn output(mut self, name: impl Into<Ident>) -> Self {
        self.output.push(name.into());
        self
    }
}
