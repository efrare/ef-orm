//! `Value` â‡„ TDS conversions â€” the only place that knows both worlds.
//!
//! Writing: [`bind_value`] pushes an [`ef_orm_core::Value`] into a
//! `tiberius::Query` as a typed parameter. Reading: [`cell_to_value`]
//! turns a `tiberius::ColumnData` cell back into a portable [`Value`].

use ef_orm_core::Value;
use ef_orm_driver::DriverError;
use tiberius::{ColumnData, IntoSql, Query};

/// Where a tiberius failure happened, so it maps onto the right
/// [`DriverError`] stratum (pattern borrowed from `sql-orm-main`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TdsErrorContext {
    ConnectTcp,
    ConfigureTcp,
    InitializeClient,
    ExecuteQuery,
    ReadCell,
}

/// Maps a tiberius error onto [`DriverError`] using its context.
pub(crate) fn map_tiberius_error(
    error: &tiberius::error::Error,
    context: TdsErrorContext,
) -> DriverError {
    use tiberius::error::Error as Tds;

    match context {
        TdsErrorContext::ConnectTcp => DriverError::Connect(format!("TCP connect failed: {error}")),
        TdsErrorContext::ConfigureTcp => DriverError::Connect(format!("TCP setup failed: {error}")),
        TdsErrorContext::InitializeClient => {
            DriverError::Connect(format!("TDS handshake failed: {error}"))
        }
        TdsErrorContext::ExecuteQuery => match error {
            // Errors the server itself reported (syntax, constraint, loginâ€¦):
            // keep the TDS error number so the facade can tell constraints
            // (2627/547/â€¦) from ordinary failures.
            Tds::Server(_) => DriverError::Server {
                code: error.code(),
                message: error.to_string(),
            },
            Tds::Io {
                kind: std::io::ErrorKind::TimedOut,
                ..
            } => DriverError::Timeout(error.to_string()),
            _ => DriverError::Protocol(error.to_string()),
        },
        TdsErrorContext::ReadCell => {
            DriverError::Protocol(format!("failed to read row value: {error}"))
        }
    }
}

/// Binds one portable value as a typed TDS parameter (`@P1`, `@P2`â€¦).
///
/// `Value::Null` is sent as an untyped `nvarchar` NULL, which SQL Server
/// implicitly converts to the target column type.
pub(crate) fn bind_value<'a>(query: &mut Query<'a>, value: &'a Value) {
    match value {
        Value::Null => query.bind(Option::<String>::None),
        Value::Bool(v) => query.bind(*v),
        Value::Int(v) => query.bind(*v),
        Value::Float(v) => query.bind(*v),
        Value::Str(v) => query.bind(v),
        Value::Bytes(v) => query.bind(v),
        // All three `IntoSql` impls are owned values (Tiberius `tds73`).
        #[cfg(feature = "with-uuid")]
        Value::Uuid(v) => query.bind(*v),
        #[cfg(feature = "with-chrono")]
        Value::DateTime(v) => query.bind(*v),
        #[cfg(feature = "with-rust_decimal")]
        Value::Decimal(v) => query.bind(*v),
    }
}

/// Owned `ColumnData` for bulk copy (the owned counterpart of [`bind_value`]).
pub(crate) fn column_data(value: &Value) -> Result<ColumnData<'static>, DriverError> {
    Ok(match value {
        Value::Null => Option::<String>::None.into_sql(),
        Value::Bool(v) => (*v).into_sql(),
        Value::Int(v) => (*v).into_sql(),
        Value::Float(v) => (*v).into_sql(),
        Value::Str(v) => v.clone().into_sql(),
        Value::Bytes(v) => v.clone().into_sql(),
        #[cfg(feature = "with-uuid")]
        Value::Uuid(v) => (*v).into_sql(),
        #[cfg(feature = "with-chrono")]
        Value::DateTime(v) => (*v).into_sql(),
        #[cfg(feature = "with-rust_decimal")]
        Value::Decimal(v) => (*v).into_sql(),
        // Feature-gated variants only exist in builds that enable them.
        #[allow(unreachable_patterns)]
        other => {
            return Err(DriverError::Protocol(format!(
                "unsupported bulk value: {other:?}"
            )));
        }
    })
}

/// Converts one result cell into a portable [`Value`].
///
/// Guid/numeric/date-time cells need their features on (`with-uuid`,
/// `with-rust_decimal`, `with-chrono`); without them â€” or for types with no
/// [`Value`] variant (`time`, `xml`, `sql_variant`â€¦) â€” the cell is rejected
/// as a protocol error (see the type map in `mssql-dialect.md`).
pub(crate) fn cell_to_value(cell: &ColumnData<'static>) -> Result<Value, DriverError> {
    Ok(match cell {
        ColumnData::U8(v) => v.map(|v| Value::Int(i64::from(v))).unwrap_or(Value::Null),
        ColumnData::I16(v) => v.map(|v| Value::Int(i64::from(v))).unwrap_or(Value::Null),
        ColumnData::I32(v) => v.map(|v| Value::Int(i64::from(v))).unwrap_or(Value::Null),
        ColumnData::I64(v) => v.map(Value::Int).unwrap_or(Value::Null),
        ColumnData::F32(v) => v.map(|v| Value::Float(f64::from(v))).unwrap_or(Value::Null),
        ColumnData::F64(v) => v.map(Value::Float).unwrap_or(Value::Null),
        ColumnData::Bit(v) => v.map(Value::Bool).unwrap_or(Value::Null),
        ColumnData::String(v) => v
            .as_ref()
            .map(|v| Value::Str(v.to_string()))
            .unwrap_or(Value::Null),
        ColumnData::Binary(v) => v
            .as_ref()
            .map(|v| Value::Bytes(v.to_vec()))
            .unwrap_or(Value::Null),
        #[cfg(feature = "with-uuid")]
        ColumnData::Guid(v) => v.map(Value::Uuid).unwrap_or(Value::Null),
        #[cfg(feature = "with-chrono")]
        ColumnData::SmallDateTime(_) | ColumnData::DateTime(_) | ColumnData::DateTime2(_) => {
            read_typed::<chrono::NaiveDateTime>(cell)?
        }
        #[cfg(feature = "with-chrono")]
        // A bare `date` has no time part: read it as `NaiveDate` and pin the
        // time to midnight (NaiveDateTime's own impl rejects `Date` cells).
        ColumnData::Date(_) => match read_cell::<chrono::NaiveDate>(cell)? {
            Some(date) => Value::DateTime(chrono::NaiveDateTime::new(date, chrono::NaiveTime::MIN)),
            None => Value::Null,
        },
        #[cfg(feature = "with-chrono")]
        // Keep the wall-clock written by the client; the offset itself is
        // dropped (a `Value` is naive â€” same contract as `Value::DateTime`).
        ColumnData::DateTimeOffset(_) => {
            match read_cell::<chrono::DateTime<chrono::FixedOffset>>(cell)? {
                Some(when) => Value::DateTime(when.naive_local()),
                None => Value::Null,
            }
        }
        #[cfg(feature = "with-rust_decimal")]
        ColumnData::Numeric(_) => read_typed::<rust_decimal::Decimal>(cell)?,
        other => {
            return Err(DriverError::Protocol(format!(
                "unsupported TDS column type: {other:?}"
            )));
        }
    })
}

/// Reads a cell through Tiberius's typed `FromSql` conversion: `Ok(None)` is
/// SQL `NULL`, an incompatible cell becomes a protocol error.
#[cfg(any(feature = "with-chrono", feature = "with-rust_decimal"))]
fn read_cell<T>(cell: &ColumnData<'static>) -> Result<Option<T>, DriverError>
where
    T: for<'a> tiberius::FromSql<'a>,
{
    <T as tiberius::FromSql<'_>>::from_sql(cell)
        .map_err(|error| DriverError::Protocol(format!("failed to convert TDS value: {error}")))
}

/// [`read_cell`] mapped straight onto a [`Value`] (`None` â‡’ `Value::Null`).
#[cfg(any(feature = "with-chrono", feature = "with-rust_decimal"))]
fn read_typed<T>(cell: &ColumnData<'static>) -> Result<Value, DriverError>
where
    T: for<'a> tiberius::FromSql<'a> + Into<Value>,
{
    Ok(read_cell::<T>(cell)?.map_or(Value::Null, Into::into))
}

#[cfg(test)]
mod tests;
