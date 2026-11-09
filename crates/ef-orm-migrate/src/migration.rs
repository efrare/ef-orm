//! A versioned migration: symmetric `up`/`down` hooks over a [`Schema`].

use ef_orm_query::Statement;

use crate::schema::Schema;

/// One versioned migration.
///
/// ```text
/// fn up(schema: &mut Schema) { … }
/// fn down(schema: &mut Schema) { … }
///
/// const V1: Migration = Migration::new("20261109_0001", "create_users", up, down);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Ordering key — migrations apply in ascending version order.
    pub version: &'static str,
    /// Human-readable description (stored in the history table).
    pub name: &'static str,
    /// Forward DDL.
    pub up: fn(&mut Schema),
    /// Reverse DDL (must undo [`Self::up`]).
    pub down: fn(&mut Schema),
}

impl Migration {
    /// Declares a migration from its two hooks.
    pub const fn new(
        version: &'static str,
        name: &'static str,
        up: fn(&mut Schema),
        down: fn(&mut Schema),
    ) -> Self {
        Self {
            version,
            name,
            up,
            down,
        }
    }

    /// The forward statements this migration queues.
    pub fn up_statements(self) -> Vec<Statement> {
        let mut schema = Schema::new();
        (self.up)(&mut schema);
        schema.statements()
    }

    /// The rollback statements this migration queues.
    pub fn down_statements(self) -> Vec<Statement> {
        let mut schema = Schema::new();
        (self.down)(&mut schema);
        schema.statements()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ef_orm_query::{ColumnDef, CreateTable, Dialect, MssqlDialect, SqlType};

    fn up(schema: &mut Schema) {
        schema.create_table(
            CreateTable::new("users")
                .column(ColumnDef::new("id", SqlType::Int).identity().primary_key())
                .column(ColumnDef::new("name", SqlType::NVarChar(50)).not_null()),
        );
    }

    fn down(schema: &mut Schema) {
        schema.drop_table("users");
    }

    const V1: Migration = Migration::new("20261109_0001", "create_users", up, down);

    fn render(stmt: &Statement) -> String {
        MssqlDialect::new().build(stmt).expect("renders").0
    }

    #[test]
    fn up_and_down_are_symmetric_hooks() {
        assert_eq!(V1.version, "20261109_0001");
        assert_eq!(V1.name, "create_users");

        let up: Vec<String> = V1.up_statements().iter().map(render).collect();
        let down: Vec<String> = V1.down_statements().iter().map(render).collect();

        assert_eq!(
            up,
            vec![
                "CREATE TABLE [dbo].[users] ([id] INT IDENTITY(1,1) NOT NULL PRIMARY KEY, \
                 [name] NVARCHAR(50) NOT NULL)"
            ]
        );
        assert_eq!(down, vec!["DROP TABLE [dbo].[users]"]);
    }
}
