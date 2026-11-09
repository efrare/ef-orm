use super::*;

#[test]
fn maps_metadata_to_portable_types() {
    assert_eq!(
        parse_sql_type("int", None, None, None).unwrap(),
        SqlType::Int
    );
    assert_eq!(
        parse_sql_type("decimal", None, Some(18), Some(2)).unwrap(),
        SqlType::Decimal {
            precision: 18,
            scale: 2
        }
    );
    assert_eq!(
        parse_sql_type("nvarchar", Some(100), None, None).unwrap(),
        SqlType::NVarChar(100)
    );
    assert_eq!(
        parse_sql_type("nvarchar", Some(-1), None, None).unwrap(),
        SqlType::NVarCharMax
    );
    assert!(parse_sql_type("geometry", None, None, None).is_err());
}

#[test]
fn primary_key_detection_uses_the_pk_constraint_set() {
    use std::sync::Arc;
    let row = |table: &str, column: &str, constraint: &str| {
        RowData::new(
            Arc::from(vec![
                "TABLE_NAME".to_string(),
                "COLUMN_NAME".to_string(),
                "CONSTRAINT_NAME".to_string(),
            ]),
            vec![
                ef_orm::Value::Str(table.into()),
                ef_orm::Value::Str(column.into()),
                ef_orm::Value::Str(constraint.into()),
            ],
        )
    };
    let constraints = vec![RowData::new(
        Arc::from(vec!["CONSTRAINT_NAME".to_string()]),
        vec![ef_orm::Value::Str("PK_users".into())],
    )];
    let keys = primary_keys(
        &[
            row("users", "id", "PK_users"),
            row("users", "email", "UQ_users_email"),
        ],
        &constraints,
    )
    .unwrap();

    assert!(keys.contains(&("users".into(), "id".into())));
    assert!(!keys.contains(&("users".into(), "email".into())));
}
