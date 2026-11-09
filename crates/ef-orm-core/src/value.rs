//! Driver-agnostic SQL value.

/// A portable SQL value shared by every layer of the ORM.
///
/// The query DSL turns [`Value`]s into **bound parameters** (never string
/// literals); the driver layer converts them to TDS types — see the type map
/// in `.opencode/skills/ef-orm/references/mssql-dialect.md`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// SQL `NULL`.
    Null,
    /// `BIT` / boolean.
    Bool(bool),
    /// Any integer (`TINYINT`…`BIGINT` are narrowed on read).
    Int(i64),
    /// `REAL` / `FLOAT`.
    Float(f64),
    /// `NVARCHAR` / `VARCHAR` text.
    Str(String),
    /// `VARBINARY` / `BINARY` data.
    Bytes(Vec<u8>),
    /// `UNIQUEIDENTIFIER` (feature `with-uuid`).
    #[cfg(feature = "with-uuid")]
    Uuid(uuid::Uuid),
    /// `datetime` / `datetime2` / `smalldatetime` (feature `with-chrono`).
    #[cfg(feature = "with-chrono")]
    DateTime(chrono::NaiveDateTime),
    /// `DECIMAL` / `NUMERIC` (feature `with-rust_decimal`).
    #[cfg(feature = "with-rust_decimal")]
    Decimal(rust_decimal::Decimal),
}

macro_rules! from_int_value {
    ($($t:ty),* $(,)?) => {
        $(
            impl From<$t> for Value {
                fn from(v: $t) -> Self {
                    Value::Int(i64::from(v))
                }
            }
        )*
    };
}

from_int_value!(i8, i16, i32, u8, u16, u32);
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Self {
        Value::Float(f64::from(v))
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_owned())
    }
}
impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}
#[cfg(feature = "with-uuid")]
impl From<uuid::Uuid> for Value {
    fn from(v: uuid::Uuid) -> Self {
        Value::Uuid(v)
    }
}
#[cfg(feature = "with-chrono")]
impl From<chrono::NaiveDateTime> for Value {
    fn from(v: chrono::NaiveDateTime) -> Self {
        Value::DateTime(v)
    }
}
#[cfg(feature = "with-rust_decimal")]
impl From<rust_decimal::Decimal> for Value {
    fn from(v: rust_decimal::Decimal) -> Self {
        Value::Decimal(v)
    }
}
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        match v {
            Some(inner) => inner.into(),
            None => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_widen_to_i64() {
        assert_eq!(Value::from(42u8), Value::Int(42));
        assert_eq!(Value::from(-1i32), Value::Int(-1));
    }

    #[test]
    fn option_maps_none_to_null() {
        assert_eq!(Value::from(Some(1i32)), Value::Int(1));
        assert_eq!(Value::from(Option::<i32>::None), Value::Null);
    }

    #[cfg(feature = "with-uuid")]
    #[test]
    fn uuid_and_option_uuid_convert_to_values() {
        let id = uuid::Uuid::from_u128(0x1234);
        assert_eq!(Value::from(id), Value::Uuid(id));
        assert_eq!(Value::from(Some(id)), Value::Uuid(id));
        assert_eq!(Value::from(Option::<uuid::Uuid>::None), Value::Null);
    }

    #[cfg(feature = "with-chrono")]
    #[test]
    fn naive_date_time_converts_to_a_value() {
        use chrono::NaiveDateTime;

        let when = chrono::NaiveDate::from_ymd_opt(2026, 11, 9)
            .and_then(|date| date.and_hms_opt(14, 30, 0))
            .expect("valid timestamp");
        assert!(matches!(when, NaiveDateTime { .. }));
        assert_eq!(Value::from(when), Value::DateTime(when));
        assert_eq!(Value::from(Some(when)), Value::DateTime(when));
        assert_eq!(Value::from(Option::<NaiveDateTime>::None), Value::Null);
    }

    #[cfg(feature = "with-rust_decimal")]
    #[test]
    fn decimal_converts_to_a_value() {
        let price = rust_decimal::Decimal::new(19_99, 2);
        assert_eq!(Value::from(price), Value::Decimal(price));
        assert_eq!(Value::from(Some(price)), Value::Decimal(price));
        assert_eq!(
            Value::from(Option::<rust_decimal::Decimal>::None),
            Value::Null
        );
    }
}
