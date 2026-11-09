//! Query-building errors.

use core::fmt;

/// Errors produced while building or rendering a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryError {
    /// The AST cannot be rendered (e.g. an `INSERT` row wider than its
    /// column list, or a statement missing a required part).
    InvalidStatement(String),
}

impl QueryError {
    /// Shorthand for [`QueryError::InvalidStatement`].
    pub fn invalid(detail: impl Into<String>) -> Self {
        Self::InvalidStatement(detail.into())
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStatement(detail) => write!(f, "invalid statement: {detail}"),
        }
    }
}

impl std::error::Error for QueryError {}

/// Convenience alias for this crate's [`Result`].
pub type QueryResult<T> = core::result::Result<T, QueryError>;
