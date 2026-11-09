use super::{TdsErrorContext, bind_value, cell_to_value, map_tiberius_error};
use ef_orm_core::Value;
use tiberius::error::Error as Tds;
use tiberius::{ColumnData, Query};

#[test]
fn reads_every_phase_one_scalar_back_as_a_value() {
    assert_eq!(
        cell_to_value(&ColumnData::I32(Some(7))).unwrap(),
        Value::Int(7)
    );
    assert_eq!(
        cell_to_value(&ColumnData::U8(Some(200))).unwrap(),
        Value::Int(200)
    );
    assert_eq!(
        cell_to_value(&ColumnData::F64(Some(1.5))).unwrap(),
        Value::Float(1.5)
    );
    assert_eq!(
        cell_to_value(&ColumnData::Bit(Some(true))).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        cell_to_value(&ColumnData::String(Some("ola".into()))).unwrap(),
        Value::Str("ola".into())
    );
    assert_eq!(
        cell_to_value(&ColumnData::Binary(Some(vec![1u8, 2].into()))).unwrap(),
        Value::Bytes(vec![1, 2])
    );
}

#[test]
fn reads_nulls_of_every_shape_as_value_null() {
    assert_eq!(cell_to_value(&ColumnData::I32(None)).unwrap(), Value::Null);
    assert_eq!(cell_to_value(&ColumnData::Bit(None)).unwrap(), Value::Null);
    assert_eq!(
        cell_to_value(&ColumnData::String(None)).unwrap(),
        Value::Null
    );
    assert_eq!(
        cell_to_value(&ColumnData::Binary(None)).unwrap(),
        Value::Null
    );
}

#[test]
fn rejects_column_types_without_a_value_variant() {
    // `time` has no `Value` variant in any phase â€” always rejected.
    let time = ColumnData::Time(None);
    let error = cell_to_value(&time).unwrap_err();

    assert!(error.to_string().starts_with("protocol error: unsupported"));
}

#[test]
fn binds_every_value_variant_as_a_parameter() {
    // `mut` pays off only when a `with-*` feature pushes its variant.
    #[allow(unused_mut)]
    let mut values = vec![
        Value::Null,
        Value::Bool(false),
        Value::Int(-9),
        Value::Float(2.5),
        Value::Str("x".into()),
        Value::Bytes(vec![9]),
    ];
    #[cfg(feature = "with-uuid")]
    values.push(Value::Uuid(tiberius::Uuid::from_u128(0xBEEF)));
    #[cfg(feature = "with-chrono")]
    values.push(Value::DateTime(sample_date_time()));
    #[cfg(feature = "with-rust_decimal")]
    values.push(Value::Decimal(rust_decimal::Decimal::new(19_99, 2)));

    let placeholders: Vec<String> = (1..=values.len()).map(|i| format!("@P{i}")).collect();
    let mut query = Query::new(format!("SELECT {}", placeholders.join(", ")));
    for value in &values {
        bind_value(&mut query, value);
    }
}

#[cfg(feature = "with-uuid")]
#[test]
fn reads_uuid_cells_back() {
    let id = tiberius::Uuid::from_u128(0x0123_4567_89ab_cdef);

    assert_eq!(
        cell_to_value(&ColumnData::Guid(Some(id))).unwrap(),
        Value::Uuid(id)
    );
    assert_eq!(cell_to_value(&ColumnData::Guid(None)).unwrap(), Value::Null);
}

#[cfg(feature = "with-chrono")]
#[test]
fn reads_date_time_cells_back() {
    use tiberius::IntoSql as _;

    let when = sample_date_time();
    // `into_sql` produces exactly the cells the server would send.
    assert_eq!(
        cell_to_value(&when.into_sql()).unwrap(),
        Value::DateTime(when)
    );
    assert_eq!(
        cell_to_value(&Option::<chrono::NaiveDateTime>::None.into_sql()).unwrap(),
        Value::Null
    );

    // A bare DATE cell reads as midnight (its NaiveDate impl).
    let midnight = chrono::NaiveDateTime::new(
        chrono::NaiveDate::from_ymd_opt(2026, 11, 9).expect("valid date"),
        chrono::NaiveTime::MIN,
    );
    let date_cell = chrono::NaiveDate::from_ymd_opt(2026, 11, 9)
        .expect("valid date")
        .into_sql();
    assert_eq!(
        cell_to_value(&date_cell).unwrap(),
        Value::DateTime(midnight)
    );
}

#[cfg(feature = "with-chrono")]
#[test]
fn reads_datetime_offset_as_wall_clock() {
    use tiberius::IntoSql as _;

    let offset = chrono::FixedOffset::east_opt(-3 * 3600).expect("valid offset");
    let utc_midday = chrono::NaiveDate::from_ymd_opt(2026, 11, 9)
        .and_then(|date| date.and_hms_opt(12, 0, 0))
        .expect("valid timestamp");
    let when =
        chrono::DateTime::<chrono::FixedOffset>::from_naive_utc_and_offset(utc_midday, offset);
    let local_nine = chrono::NaiveDate::from_ymd_opt(2026, 11, 9)
        .and_then(|date| date.and_hms_opt(9, 0, 0))
        .expect("valid timestamp");

    // 12:00 UTC at UTC-3 â‡’ wall-clock 09:00 kept, only the offset dropped.
    assert_eq!(
        cell_to_value(&when.into_sql()).unwrap(),
        Value::DateTime(local_nine)
    );
}

#[cfg(feature = "with-rust_decimal")]
#[test]
fn reads_decimal_cells_back() {
    use tiberius::IntoSql as _;

    let price = rust_decimal::Decimal::new(19_99, 2);
    assert_eq!(
        cell_to_value(&price.into_sql()).unwrap(),
        Value::Decimal(price)
    );
    assert_eq!(
        cell_to_value(&Option::<rust_decimal::Decimal>::None.into_sql()).unwrap(),
        Value::Null
    );
}

#[cfg(feature = "with-chrono")]
fn sample_date_time() -> chrono::NaiveDateTime {
    chrono::NaiveDate::from_ymd_opt(2026, 11, 9)
        .and_then(|date| date.and_hms_opt(14, 30, 0))
        .expect("valid timestamp")
}

#[test]
fn maps_execution_failures_by_error_kind() {
    let conversion = Tds::Conversion("bad cast".into());
    assert!(matches!(
        map_tiberius_error(&conversion, TdsErrorContext::ExecuteQuery),
        ef_orm_driver::DriverError::Protocol(_)
    ));

    let timed_out = Tds::Io {
        kind: std::io::ErrorKind::TimedOut,
        message: "deadline".into(),
    };
    assert!(matches!(
        map_tiberius_error(&timed_out, TdsErrorContext::ExecuteQuery),
        ef_orm_driver::DriverError::Timeout(_)
    ));
}

#[test]
fn maps_session_setup_failures_to_the_connect_stratum() {
    let error = Tds::Protocol("bad packet".into());

    assert!(matches!(
        map_tiberius_error(&error, TdsErrorContext::InitializeClient),
        ef_orm_driver::DriverError::Connect(_)
    ));
    assert!(matches!(
        map_tiberius_error(&error, TdsErrorContext::ConnectTcp),
        ef_orm_driver::DriverError::Connect(_)
    ));
    assert!(matches!(
        map_tiberius_error(&error, TdsErrorContext::ReadCell),
        ef_orm_driver::DriverError::Protocol(_)
    ));
}
