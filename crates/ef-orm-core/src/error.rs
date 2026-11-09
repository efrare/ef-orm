//! Domain errors of **ef-ORM**.
//!
//! Transport/infra failures live in `ef_orm_driver::DriverError`; the facade
//! converts them into these domain errors at the boundary, so end users only
//! ever see [`OrmError`] (see `.opencode/skills/ef-orm/references/conventions.md`).

/// Domain-level errors visible to ORM users.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OrmError {
    /// The expected record/row does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// A value could not be converted to/from the requested type, or a
    /// column could not be mapped.
    #[error("conversion failed: {0}")]
    Conversion(String),
    /// A database constraint was violated (unique, FK, check, not null).
    #[error("constraint violation: {0}")]
    Constraint(String),
    /// A transaction could not be started or completed.
    #[error("transaction error: {0}")]
    Transaction(String),
    /// Optimistic concurrency check failed (concurrent modification).
    #[error("concurrency conflict: {0}")]
    Conflict(String),
    /// The API was used in an invalid way (e.g. resolving a staged insert
    /// before `save_changes` ran, or a caller contract violation).
    #[error("validation error: {0}")]
    Validation(String),
    /// The database or its driver failed: connection, timeout, protocol or a
    /// server error that is not a constraint violation. Raised by the facade
    /// when it converts `DriverError` at the boundary.
    #[error("database error: {0}")]
    Database(String),
}

/// Convenience alias used across the workspace.
pub type Result<T> = std::result::Result<T, OrmError>;

#[cfg(test)]
mod tests {
    use super::{OrmError, Result};

    #[test]
    fn displays_the_failure_kind_and_detail() {
        assert_eq!(
            OrmError::NotFound("User#42".into()).to_string(),
            "not found: User#42"
        );
        assert_eq!(
            OrmError::Conversion("i32 from Str".into()).to_string(),
            "conversion failed: i32 from Str"
        );
        assert_eq!(
            OrmError::Constraint("UNIQUE KEY".into()).to_string(),
            "constraint violation: UNIQUE KEY"
        );
        assert_eq!(
            OrmError::Transaction("already committed".into()).to_string(),
            "transaction error: already committed"
        );
        assert_eq!(
            OrmError::Conflict("User#42".into()).to_string(),
            "concurrency conflict: User#42"
        );
        assert_eq!(
            OrmError::Validation("resolve before save_changes".into()).to_string(),
            "validation error: resolve before save_changes"
        );
        assert_eq!(
            OrmError::Database("connection refused".into()).to_string(),
            "database error: connection refused"
        );
    }

    #[test]
    fn result_alias_carries_orm_errors() {
        fn fail() -> Result<()> {
            Err(OrmError::NotFound("id=1".into()))
        }

        assert_eq!(fail().unwrap_err(), OrmError::NotFound("id=1".into()));
    }
}
