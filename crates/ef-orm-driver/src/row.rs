//! Typed row access shared by every driver.
//!
//! Drivers materialize result sets into [`RowData`] (portable [`Value`]s);
//! [`Row`] is the accessor trait future streaming rows will also implement.

use ef_orm_core::{OrmError, Value};
use std::sync::Arc;

/// A single result row: column names plus one [`Value`] per cell.
///
/// Column names are shared (cheap `Arc` clone) across the rows of a result
/// set and matched case-insensitively, mirroring SQL Server's default
/// collation.
#[derive(Debug, Clone, PartialEq)]
pub struct RowData {
    columns: Arc<[String]>,
    values: Vec<Value>,
}

impl RowData {
    /// Builds a row; `columns` and `values` must have the same length.
    pub fn new(columns: Arc<[String]>, values: Vec<Value>) -> Self {
        Self { columns, values }
    }
}

impl Row for RowData {
    fn columns(&self) -> &[String] {
        &self.columns
    }

    fn value_at(&self, idx: usize) -> Result<&Value, OrmError> {
        self.values.get(idx).ok_or_else(|| {
            OrmError::Conversion(format!(
                "row index {idx} out of range ({} columns)",
                self.columns.len()
            ))
        })
    }

    fn value_of(&self, name: &str) -> Result<&Value, OrmError> {
        let idx = self
            .columns
            .iter()
            .position(|column| column.eq_ignore_ascii_case(name))
            .ok_or_else(|| OrmError::Conversion(format!("row has no column '{name}'")))?;
        self.value_at(idx)
    }
}

/// Accessors over a result row: raw [`Value`]s plus typed getters.
pub trait Row: Send {
    /// Column names in ordinal order.
    fn columns(&self) -> &[String];

    /// Raw cell by ordinal position (0-based).
    fn value_at(&self, idx: usize) -> Result<&Value, OrmError>;

    /// Raw cell by column name (case-insensitive).
    fn value_of(&self, name: &str) -> Result<&Value, OrmError>;

    /// Typed cell by ordinal position: `None` means SQL `NULL`.
    ///
    /// Missing columns and incompatible types surface as
    /// [`OrmError::Conversion`].
    fn get<T: FromValue>(&self, idx: usize) -> Result<Option<T>, OrmError> {
        match self.value_at(idx)? {
            Value::Null => Ok(None),
            value => T::from_value(value).map(Some),
        }
    }

    /// Typed cell by column name: `None` means SQL `NULL`.
    fn get_by_name<T: FromValue>(&self, name: &str) -> Result<Option<T>, OrmError> {
        match self.value_of(name)? {
            Value::Null => Ok(None),
            value => T::from_value(value).map(Some),
        }
    }
}

/// Lossless(ish) conversion from a [`Value`] cell into a concrete Rust type.
///
/// Only implemented for the phase-1 scalar types; `Uuid`, `chrono` and
/// `decimal` arrive with the entity phase.
pub trait FromValue: Sized {
    /// Converts a non-NULL [`Value`], rejecting incompatible shapes.
    fn from_value(value: &Value) -> Result<Self, OrmError>;
}

/// Conversion of a result row into a typed value — how entities are read.
///
/// Implemented by `#[derive(Model)]` (re-exported from the facade as
/// `ef_orm::FromRow`). Mapping reads columns **by name**, so it is robust to
/// projection order and `SELECT *`.
pub trait FromRow: Sized {
    /// Builds `Self` from one row; a NULL in a non-`Option` field surfaces
    /// as [`OrmError::Conversion`].
    fn from_row<R: Row>(row: &R) -> Result<Self, OrmError>;
}

impl FromValue for Value {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        Ok(value.clone())
    }
}

impl FromValue for bool {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        match value {
            Value::Bool(v) => Ok(*v),
            other => Err(type_mismatch("bool", other)),
        }
    }
}

macro_rules! from_value_int {
    ($($t:ty),* $(,)?) => {
        $(
            impl FromValue for $t {
                fn from_value(value: &Value) -> Result<Self, OrmError> {
                    match value {
                        Value::Int(v) => <$t>::try_from(*v).map_err(|_| {
                            OrmError::Conversion(format!("{v} out of range for {}", stringify!($t)))
                        }),
                        other => Err(type_mismatch(stringify!($t), other)),
                    }
                }
            }
        )*
    };
}

from_value_int!(i8, i16, i32, i64, u8, u16, u32);

impl FromValue for f32 {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        match value {
            Value::Float(v) => Ok(*v as f32),
            other => Err(type_mismatch("f32", other)),
        }
    }
}

impl FromValue for f64 {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        match value {
            Value::Float(v) => Ok(*v),
            other => Err(type_mismatch("f64", other)),
        }
    }
}

impl FromValue for String {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        match value {
            Value::Str(v) => Ok(v.clone()),
            other => Err(type_mismatch("String", other)),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        match value {
            Value::Bytes(v) => Ok(v.clone()),
            other => Err(type_mismatch("Vec<u8>", other)),
        }
    }
}

#[cfg(feature = "with-uuid")]
impl FromValue for uuid::Uuid {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        match value {
            Value::Uuid(v) => Ok(*v),
            other => Err(type_mismatch("Uuid", other)),
        }
    }
}

#[cfg(feature = "with-chrono")]
impl FromValue for chrono::NaiveDateTime {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        match value {
            Value::DateTime(v) => Ok(*v),
            other => Err(type_mismatch("NaiveDateTime", other)),
        }
    }
}

#[cfg(feature = "with-rust_decimal")]
impl FromValue for rust_decimal::Decimal {
    fn from_value(value: &Value) -> Result<Self, OrmError> {
        match value {
            Value::Decimal(v) => Ok(*v),
            other => Err(type_mismatch("Decimal", other)),
        }
    }
}

fn type_mismatch(expected: &str, found: &Value) -> OrmError {
    OrmError::Conversion(format!("expected {expected}, found {found:?}"))
}

#[cfg(test)]
mod tests {
    use super::{FromValue, Row, RowData};
    use ef_orm_core::{OrmError, Value};
    use std::sync::Arc;

    fn sample_row() -> RowData {
        RowData::new(
            Arc::from(vec![
                "Id".to_string(),
                "Name".to_string(),
                "Age".to_string(),
            ]),
            vec![Value::Int(7), Value::Str("ana".into()), Value::Null],
        )
    }

    #[test]
    fn gets_typed_values_by_index_and_name() {
        let row = sample_row();

        assert_eq!(row.get::<i32>(0).unwrap(), Some(7));
        assert_eq!(row.get::<String>(1).unwrap(), Some("ana".into()));
        // SQL NULL reads as None…
        assert_eq!(row.get::<i32>(2).unwrap(), None);
        // …and names are matched case-insensitively.
        assert_eq!(row.get_by_name::<i64>("id").unwrap(), Some(7));
    }

    #[test]
    fn missing_column_and_index_are_conversion_errors() {
        let row = sample_row();

        assert_eq!(
            row.get::<i32>(9).unwrap_err(),
            OrmError::Conversion("row index 9 out of range (3 columns)".into())
        );
        assert_eq!(
            row.get_by_name::<i32>("nope").unwrap_err(),
            OrmError::Conversion("row has no column 'nope'".into())
        );
    }

    #[test]
    fn rejects_incompatible_types_and_out_of_range_integers() {
        let row = sample_row();

        assert!(matches!(
            row.get::<String>(0).unwrap_err(),
            OrmError::Conversion(_)
        ));
        let big = RowData::new(
            Arc::from(vec!["v".to_string()]),
            vec![Value::Int(i64::from(u32::MAX) + 1)],
        );
        assert_eq!(
            big.get::<u32>(0).unwrap_err(),
            OrmError::Conversion("4294967296 out of range for u32".into())
        );
    }

    #[test]
    fn from_value_covers_the_phase_one_scalars() {
        assert_eq!(Value::from_value(&Value::Int(1)).unwrap(), Value::Int(1));
        assert!(bool::from_value(&Value::Bool(true)).unwrap());
        assert_eq!(i64::from_value(&Value::Int(-2)).unwrap(), -2);
        assert_eq!(f64::from_value(&Value::Float(0.5)).unwrap(), 0.5);
        assert_eq!(
            Vec::<u8>::from_value(&Value::Bytes(vec![1, 2])).unwrap(),
            vec![1, 2]
        );
        assert!(f32::from_value(&Value::Str("x".into())).is_err());
    }

    #[cfg(feature = "with-uuid")]
    #[test]
    fn from_value_reads_uuids_and_rejects_other_shapes() {
        let id = uuid::Uuid::from_u128(0xBEEF);
        assert_eq!(
            <uuid::Uuid as FromValue>::from_value(&Value::Uuid(id)).unwrap(),
            id
        );
        assert!(matches!(
            <uuid::Uuid as FromValue>::from_value(&Value::Str(id.to_string())),
            Err(OrmError::Conversion(_))
        ));
    }

    #[cfg(feature = "with-chrono")]
    #[test]
    fn from_value_reads_naive_date_times() {
        let when = chrono::NaiveDate::from_ymd_opt(2026, 11, 9)
            .and_then(|date| date.and_hms_opt(8, 0, 0))
            .expect("valid timestamp");
        assert_eq!(
            <chrono::NaiveDateTime as FromValue>::from_value(&Value::DateTime(when)).unwrap(),
            when
        );
        assert!(matches!(
            <chrono::NaiveDateTime as FromValue>::from_value(&Value::Int(0)),
            Err(OrmError::Conversion(_))
        ));
    }

    #[cfg(feature = "with-rust_decimal")]
    #[test]
    fn from_value_reads_decimals() {
        let price = rust_decimal::Decimal::new(12_345, 2);
        assert_eq!(
            <rust_decimal::Decimal as FromValue>::from_value(&Value::Decimal(price)).unwrap(),
            price
        );
        assert!(matches!(
            <rust_decimal::Decimal as FromValue>::from_value(&Value::Float(1.0)),
            Err(OrmError::Conversion(_))
        ));
    }

    #[test]
    fn from_row_maps_columns_by_name() {
        use super::FromRow;

        struct Picked {
            name: String,
        }

        impl FromRow for Picked {
            fn from_row<R: Row>(row: &R) -> Result<Self, OrmError> {
                Ok(Self {
                    name: row
                        .get_by_name("name")?
                        .ok_or_else(|| OrmError::Conversion("name is NULL".into()))?,
                })
            }
        }

        let row = sample_row(); // columns: Id, Name, Age
        let picked = Picked::from_row(&row).unwrap();
        assert_eq!(picked.name, "ana");
    }
}
