//! Tests for the code `#[derive(Entity)]`/`#[derive(Model)]` generate.
//!
//! No database involved: metadata, `Row → struct` mapping (columns read by
//! name), insert value lists and identity write-back are all pure.

use std::sync::Arc;

use ef_orm_query::Expr;

use crate::{Column, Entity, FromRow, Model, OrmError, RowData, Value};

#[derive(Entity, Model, Debug)]
#[orm(table = "users", schema = "auth")]
struct User {
    #[orm(primary_key, auto)]
    id: i32,
    name: String,
    nickname: Option<String>,
    active: bool,
}

/// Builds a row with **shuffled** column order — mapping must go by name.
fn row(id: Value, name: Value, nickname: Value, active: Value) -> RowData {
    RowData::new(
        Arc::from(vec![
            "nickname".to_string(),
            "active".to_string(),
            "name".to_string(),
            "id".to_string(),
        ]),
        vec![nickname, active, name, id],
    )
}

#[test]
fn entity_metadata_comes_from_the_attributes() {
    assert_eq!(User::TABLE, "users");
    assert_eq!(User::SCHEMA, Some("auth"));
    assert_eq!(User::COLUMNS, &["id", "name", "nickname", "active"]);
    assert_eq!(User::PRIMARY_KEY, "id");
    // Compile-time: the attribute maps onto a `const`.
    const {
        assert!(User::PRIMARY_KEY_AUTO);
    }

    let id: i32 = User {
        id: 1,
        name: String::new(),
        nickname: None,
        active: true,
    }
    .id();
    assert_eq!(id, 1);
}

#[test]
fn generated_columns_build_typed_predicates() {
    let columns = User::columns();

    assert_eq!(columns.name.name(), "name");
    assert_eq!(
        columns.name.like("A%"),
        Expr::col("name").like(Expr::val("A%"))
    );
    assert_eq!(
        columns.active.eq(true),
        Expr::col("active").eq(Expr::val(true))
    );
    // `nickname` is `Option<String>` ⇒ nullable text, still a text column.
    assert_eq!(columns.nickname.is_null(), Expr::col("nickname").is_null());
}

#[test]
fn from_row_maps_by_name_and_treats_null_as_option() {
    let user = User::from_row(&row(
        Value::Int(7),
        Value::Str("ada".into()),
        Value::Null,
        Value::Bool(true),
    ))
    .expect("maps");

    assert_eq!(user.id, 7);
    assert_eq!(user.name, "ada");
    assert_eq!(user.nickname, None);
    assert!(user.active);
}

#[test]
fn from_row_rejects_null_in_a_required_field() {
    let error = User::from_row(&row(
        Value::Int(7),
        Value::Null, // name is required
        Value::Null,
        Value::Bool(true),
    ))
    .unwrap_err();

    assert!(matches!(error, OrmError::Conversion(_)));
    assert!(error.to_string().contains("name"));
}

#[test]
fn to_values_omits_the_identity_key_and_keeps_nullables() {
    let user = User {
        id: 42,
        name: "ada".into(),
        nickname: None,
        active: false,
    };

    assert_eq!(
        user.to_values(),
        vec![
            ("name", Value::Str("ada".into())),
            ("nickname", Value::Null),
            ("active", Value::Bool(false)),
        ]
    );
}

#[test]
fn set_id_writes_the_server_generated_key_back() {
    let mut user = User {
        id: 0,
        name: "ada".into(),
        nickname: None,
        active: true,
    };

    user.set_id(Value::Int(9)).expect("int fits i32");
    assert_eq!(user.id(), 9);

    let error = user.set_id(Value::Str("nope".into())).unwrap_err();
    assert!(matches!(error, OrmError::Conversion(_)));
}

#[test]
fn natural_keys_are_inserted_and_keep_their_value() {
    #[derive(Entity, Model)]
    #[orm(table = "codes")]
    struct Code {
        #[orm(primary_key)]
        code: String,
        label: String,
    }

    let code = Code {
        code: "BR-1".into(),
        label: "Brazil".into(),
    };

    const {
        assert!(!Code::PRIMARY_KEY_AUTO);
    }
    assert_eq!(
        code.to_values(),
        vec![
            ("code", Value::Str("BR-1".into())),
            ("label", Value::Str("Brazil".into())),
        ]
    );
}

/// Derives over the optional column types: the generated `Value::from` /
/// `FromValue` paths for uuid/chrono/decimal need every `with-*` feature.
#[cfg(all(
    feature = "with-uuid",
    feature = "with-chrono",
    feature = "with-rust_decimal"
))]
mod typed {
    use super::*;

    #[derive(Entity, Model, Debug)]
    #[orm(table = "invoices")]
    struct Invoice {
        #[orm(primary_key, auto)]
        id: i64,
        trace_id: uuid::Uuid,
        issued_at: chrono::NaiveDateTime,
        total: rust_decimal::Decimal,
        paid_at: Option<chrono::NaiveDateTime>,
    }

    fn when() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 11, 9)
            .and_then(|date| date.and_hms_opt(14, 30, 0))
            .expect("valid timestamp")
    }

    fn invoice() -> Invoice {
        Invoice {
            id: 1,
            trace_id: uuid::Uuid::from_u128(0xCAFE),
            issued_at: when(),
            total: rust_decimal::Decimal::new(19_99, 2),
            paid_at: None,
        }
    }

    #[test]
    fn to_values_carries_uuid_datetime_and_decimal() {
        assert_eq!(
            invoice().to_values(),
            vec![
                ("trace_id", Value::Uuid(uuid::Uuid::from_u128(0xCAFE))),
                ("issued_at", Value::DateTime(when())),
                (
                    "total",
                    Value::Decimal(rust_decimal::Decimal::new(19_99, 2))
                ),
                ("paid_at", Value::Null),
            ]
        );
    }

    #[test]
    fn from_row_reads_the_optional_types_by_name() {
        // Shuffled order on purpose: mapping goes by column name.
        let row = RowData::new(
            Arc::from(vec![
                "paid_at".to_string(),
                "total".to_string(),
                "issued_at".to_string(),
                "trace_id".to_string(),
                "id".to_string(),
            ]),
            vec![
                Value::Null,
                Value::Decimal(rust_decimal::Decimal::new(19_99, 2)),
                Value::DateTime(when()),
                Value::Uuid(uuid::Uuid::from_u128(0xCAFE)),
                Value::Int(7),
            ],
        );

        let mut invoice = Invoice::from_row(&row).expect("maps");
        assert_eq!(invoice.id, 7);
        assert_eq!(invoice.trace_id, uuid::Uuid::from_u128(0xCAFE));
        assert_eq!(invoice.issued_at, when());
        assert_eq!(invoice.total, rust_decimal::Decimal::new(19_99, 2));
        assert_eq!(invoice.paid_at, None);

        invoice.set_id(Value::Int(8)).expect("i64 fits");
        assert_eq!(invoice.id, 8);
    }
}
