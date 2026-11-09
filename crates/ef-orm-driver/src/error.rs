//! Infrastructure errors of the transport layer.

/// Infra errors raised by drivers and the pool.
///
/// The facade converts these into `ef_orm_core::OrmError` at the boundary;
/// end users only see domain errors (see
/// `.opencode/skills/ef-orm/references/conventions.md`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriverError {
    /// Could not open a connection (TCP, TLS, auth, routing).
    #[error("connection failed: {0}")]
    Connect(String),
    /// An operation exceeded its deadline (connect, pool acquire, query).
    #[error("timed out: {0}")]
    Timeout(String),
    /// The wire protocol was violated or a value could not be mapped.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// The server rejected the statement (SQL error: syntax, constraint…).
    /// `code` is the TDS error number when the server sent one (e.g. 2627
    /// for a PK/unique violation, 547 for FK/check) — the facade uses it to
    /// map onto `OrmError::Constraint`.
    #[error("query failed: {message}")]
    Server {
        /// Server error number, when known.
        code: Option<u32>,
        /// Human-readable server message.
        message: String,
    },
    /// The connection or pool is closed and cannot serve requests.
    #[error("connection closed")]
    Closed,
}

#[cfg(test)]
mod tests {
    use super::DriverError;

    #[test]
    fn displays_the_failure_kind_and_detail() {
        assert_eq!(
            DriverError::Connect("refused".into()).to_string(),
            "connection failed: refused"
        );
        assert_eq!(
            DriverError::Timeout("30s".into()).to_string(),
            "timed out: 30s"
        );
        assert_eq!(
            DriverError::Protocol("bad token".into()).to_string(),
            "protocol error: bad token"
        );
        assert_eq!(
            DriverError::Server {
                code: Some(2627),
                message: "duplicate key".into()
            }
            .to_string(),
            "query failed: duplicate key"
        );
        assert_eq!(
            DriverError::Server {
                code: None,
                message: "syntax error".into()
            }
            .to_string(),
            "query failed: syntax error"
        );
        assert_eq!(DriverError::Closed.to_string(), "connection closed");
    }
}
