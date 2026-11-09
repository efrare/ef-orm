//! Boundary conversions: infra errors → domain errors.
//!
//! Per `references/conventions.md`, users only ever see `OrmError`; this is
//! the single place where `DriverError` crosses into the domain.

use ef_orm_core::OrmError;
use ef_orm_driver::DriverError;

/// SQL Server error numbers meaning "a constraint was violated":
/// 2601 (unique index), 2627 (PK/unique constraint), 547 (FK/check),
/// 515 (NOT NULL).
const CONSTRAINT_CODES: [u32; 4] = [2601, 2627, 547, 515];

/// SQL Server error numbers meaning "a value could not be converted":
/// 241/245 (conversion failed), 8114/8115 (error converting data type).
const CONVERSION_CODES: [u32; 4] = [241, 245, 8114, 8115];

/// Converts an infra error into the domain error shown to users.
pub(crate) fn map_driver_error(error: DriverError) -> OrmError {
    match error {
        DriverError::Server {
            code: Some(code),
            message,
        } if CONSTRAINT_CODES.contains(&code) => OrmError::Constraint(message),
        DriverError::Server {
            code: Some(code),
            message,
        } if CONVERSION_CODES.contains(&code) => OrmError::Conversion(message),
        DriverError::Server { message, .. } => OrmError::Database(message),
        other => OrmError::Database(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::map_driver_error;
    use ef_orm_core::OrmError;
    use ef_orm_driver::DriverError;

    #[test]
    fn server_constraint_codes_become_constraint_errors() {
        for code in [2601, 2627, 547, 515] {
            let error = map_driver_error(DriverError::Server {
                code: Some(code),
                message: format!("violation {code}"),
            });
            assert!(
                matches!(error, OrmError::Constraint(_)),
                "code {code} should map to Constraint, got {error:?}"
            );
        }
    }

    #[test]
    fn server_conversion_codes_become_conversion_errors() {
        let error = map_driver_error(DriverError::Server {
            code: Some(245),
            message: "Conversion failed".into(),
        });
        assert!(matches!(error, OrmError::Conversion(_)));
    }

    #[test]
    fn other_server_errors_and_transport_failures_fall_back_to_database() {
        assert!(matches!(
            map_driver_error(DriverError::Server {
                code: Some(102),
                message: "syntax error".into()
            }),
            OrmError::Database(_)
        ));
        assert!(matches!(
            map_driver_error(DriverError::Server {
                code: None,
                message: "login failed".into()
            }),
            OrmError::Database(_)
        ));
        assert!(matches!(
            map_driver_error(DriverError::Closed),
            OrmError::Database(_)
        ));
        assert!(matches!(
            map_driver_error(DriverError::Connect("refused".into())),
            OrmError::Database(_)
        ));
        assert!(matches!(
            map_driver_error(DriverError::Timeout("30s".into())),
            OrmError::Database(_)
        ));
    }

    #[test]
    fn domain_messages_keep_the_failure_detail() {
        let error = map_driver_error(DriverError::Server {
            code: Some(2627),
            message: "PRIMARY KEY constraint violated".into(),
        });
        assert_eq!(
            error.to_string(),
            "constraint violation: PRIMARY KEY constraint violated"
        );
    }
}
