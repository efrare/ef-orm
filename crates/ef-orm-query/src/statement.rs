//! Top-level statement wrapper dispatched to a [`Dialect`](crate::Dialect).

use crate::ddl::{
    AddColumn, AddForeignKey, AlterColumn, CreateIndex, CreateTable, DropColumn, DropConstraint,
    DropIndex, DropTable,
};
use crate::delete::DeleteStatement;
use crate::insert::InsertStatement;
use crate::select::SelectStatement;
use crate::update::UpdateStatement;

/// Any renderable statement: DML (`Select`/`Insert`/`Update`/`Delete`) plus
/// the DDL used by migrations.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Select(SelectStatement),
    Insert(InsertStatement),
    Update(UpdateStatement),
    Delete(DeleteStatement),
    CreateTable(CreateTable),
    DropTable(DropTable),
    AddColumn(AddColumn),
    DropColumn(DropColumn),
    AlterColumn(AlterColumn),
    CreateIndex(CreateIndex),
    DropIndex(DropIndex),
    AddForeignKey(AddForeignKey),
    DropConstraint(DropConstraint),
}

impl From<SelectStatement> for Statement {
    fn from(s: SelectStatement) -> Self {
        Self::Select(s)
    }
}

impl From<InsertStatement> for Statement {
    fn from(s: InsertStatement) -> Self {
        Self::Insert(s)
    }
}

impl From<UpdateStatement> for Statement {
    fn from(s: UpdateStatement) -> Self {
        Self::Update(s)
    }
}

impl From<DeleteStatement> for Statement {
    fn from(s: DeleteStatement) -> Self {
        Self::Delete(s)
    }
}

impl From<CreateTable> for Statement {
    fn from(s: CreateTable) -> Self {
        Self::CreateTable(s)
    }
}

impl From<DropTable> for Statement {
    fn from(s: DropTable) -> Self {
        Self::DropTable(s)
    }
}

impl From<AddColumn> for Statement {
    fn from(s: AddColumn) -> Self {
        Self::AddColumn(s)
    }
}

impl From<DropColumn> for Statement {
    fn from(s: DropColumn) -> Self {
        Self::DropColumn(s)
    }
}

impl From<AlterColumn> for Statement {
    fn from(s: AlterColumn) -> Self {
        Self::AlterColumn(s)
    }
}

impl From<CreateIndex> for Statement {
    fn from(s: CreateIndex) -> Self {
        Self::CreateIndex(s)
    }
}

impl From<DropIndex> for Statement {
    fn from(s: DropIndex) -> Self {
        Self::DropIndex(s)
    }
}

impl From<AddForeignKey> for Statement {
    fn from(s: AddForeignKey) -> Self {
        Self::AddForeignKey(s)
    }
}

impl From<DropConstraint> for Statement {
    fn from(s: DropConstraint) -> Self {
        Self::DropConstraint(s)
    }
}
