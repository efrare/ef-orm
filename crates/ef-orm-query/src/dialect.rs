//! The dialect extension point: AST → engine-specific SQL.

use ef_orm_core::Value;

use crate::error::QueryResult;
use crate::statement::Statement;

/// Renders a [`Statement`] into SQL text plus bound parameters.
///
/// The only crate allowed to produce SQL strings is `ef-orm-query`; adding a
/// new database means implementing this trait (see `MssqlDialect`).
pub trait Dialect {
    /// Quotes an identifier for this engine (T-SQL: `[name]`).
    fn quote_ident(&self, ident: &str) -> String;

    /// Placeholder for the n-th bound parameter, 1-based (T-SQL: `@P1`).
    fn placeholder(&self, index: usize) -> String;

    /// Schema applied when a table carries none (T-SQL: `dbo`).
    fn default_schema(&self) -> Option<&str> {
        None
    }

    /// Renders a full statement: SQL text with placeholders + parameter values
    /// in placeholder order.
    fn build(&self, stmt: &Statement) -> QueryResult<(String, Vec<Value>)>;
}
