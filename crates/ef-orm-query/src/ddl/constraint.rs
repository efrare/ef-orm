//! Foreign keys and constraint drops.

use crate::ident::{Ident, Table};

/// Referential action for `ON DELETE` / `ON UPDATE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefAction {
    Cascade,
    SetNull,
    SetDefault,
    NoAction,
    Restrict,
}

impl RefAction {
    /// The T-SQL keyword pair (`CASCADE`, `SET NULL`, â€¦).
    pub fn as_sql(&self) -> &'static str {
        match self {
            Self::Cascade => "CASCADE",
            Self::SetNull => "SET NULL",
            Self::SetDefault => "SET DEFAULT",
            Self::NoAction => "NO ACTION",
            Self::Restrict => "RESTRICT",
        }
    }
}

/// `ALTER TABLE â€¦ ADD CONSTRAINT â€¦ FOREIGN KEY â€¦ REFERENCES â€¦`.
#[derive(Debug, Clone, PartialEq)]
pub struct AddForeignKey {
    pub name: Ident,
    pub table: Table,
    pub columns: Vec<Ident>,
    pub ref_table: Table,
    pub ref_columns: Vec<Ident>,
    pub on_delete: Option<RefAction>,
    pub on_update: Option<RefAction>,
}

impl AddForeignKey {
    pub fn new(
        name: impl Into<Ident>,
        table: impl Into<Table>,
        ref_table: impl Into<Table>,
    ) -> Self {
        Self {
            name: name.into(),
            table: table.into(),
            columns: Vec::new(),
            ref_table: ref_table.into(),
            ref_columns: Vec::new(),
            on_delete: None,
            on_update: None,
        }
    }

    pub fn column(mut self, column: impl Into<Ident>) -> Self {
        self.columns.push(column.into());
        self
    }

    pub fn references(mut self, column: impl Into<Ident>) -> Self {
        self.ref_columns.push(column.into());
        self
    }

    pub fn on_delete(mut self, action: RefAction) -> Self {
        self.on_delete = Some(action);
        self
    }

    pub fn on_update(mut self, action: RefAction) -> Self {
        self.on_update = Some(action);
        self
    }
}

/// `ALTER TABLE â€¦ DROP CONSTRAINT â€¦`.
#[derive(Debug, Clone, PartialEq)]
pub struct DropConstraint {
    pub table: Table,
    pub name: Ident,
}

impl DropConstraint {
    pub fn new(table: impl Into<Table>, name: impl Into<Ident>) -> Self {
        Self {
            table: table.into(),
            name: name.into(),
        }
    }
}
