//! Parsing of the `#[orm(...)]` attributes shared by both derives.
//!
//! ```text
//! #[orm(table = "users", schema = "dbo")]   // struct level (Entity)
//! struct User {
//!     #[orm(primary_key, auto, column = "id")]  // field level
//!     id: i32,
//! }
//! ```

use syn::{Attribute, Data, DeriveInput, Error, Fields, Ident, LitStr, Type};

/// Struct-level attributes.
#[derive(Debug, Default)]
pub(crate) struct StructAttrs {
    pub table: Option<String>,
    pub schema: Option<String>,
}

/// Field-level attributes.
#[derive(Debug, Default)]
pub(crate) struct FieldAttrs {
    /// The field carries the (single) primary key.
    pub primary_key: bool,
    /// The key is `IDENTITY`: omitted from `INSERT`, read from `OUTPUT`.
    pub auto: bool,
    /// Overrides the column name (defaults to the field name).
    pub column: Option<String>,
}

/// One mapped field: identity, Rust type, column name and attributes.
pub(crate) struct FieldInfo<'a> {
    pub ident: &'a Ident,
    pub ty: &'a Type,
    /// SQL column name (attribute override or the field name).
    pub name: String,
    pub attrs: FieldAttrs,
}

impl FieldInfo<'_> {
    /// Whether the field is written as `Option<…>` (nullable column).
    pub fn is_option(&self) -> bool {
        match self.ty {
            Type::Path(path) => path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Option"),
            _ => false,
        }
    }
}

/// Parses `#[orm(...)]` on the struct (unknown keys are errors).
pub(crate) fn parse_struct(attrs: &[Attribute]) -> syn::Result<StructAttrs> {
    let mut parsed = StructAttrs::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("orm")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                parsed.table = Some(lit_str(&meta)?);
            } else if meta.path.is_ident("schema") {
                parsed.schema = Some(lit_str(&meta)?);
            } else {
                return Err(meta.error("unknown #[orm(...)] key (expected table or schema)"));
            }
            Ok(())
        })?;
    }
    Ok(parsed)
}

/// Parses `#[orm(...)]` on one field.
pub(crate) fn parse_field(attrs: &[Attribute]) -> syn::Result<FieldAttrs> {
    let mut parsed = FieldAttrs::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("orm")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("primary_key") {
                parsed.primary_key = true;
            } else if meta.path.is_ident("auto") {
                parsed.auto = true;
            } else if meta.path.is_ident("column") {
                parsed.column = Some(lit_str(&meta)?);
            } else {
                return Err(
                    meta.error("unknown #[orm(...)] key (expected primary_key, auto or column)")
                );
            }
            Ok(())
        })?;
    }
    if parsed.auto && !parsed.primary_key {
        return Err(Error::new(
            attrs_span(attrs),
            "`auto` requires `primary_key` on the same field",
        ));
    }
    Ok(parsed)
}

/// Collects the named fields of a derive input as [`FieldInfo`]s.
pub(crate) fn field_infos(input: &DeriveInput) -> syn::Result<Vec<FieldInfo<'_>>> {
    let Data::Struct(data) = &input.data else {
        return Err(Error::new(
            input.ident.span(),
            "ef-ORM entities must be structs with named fields",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new(
            input.ident.span(),
            "ef-ORM entities must be structs with named fields",
        ));
    };
    collect(fields)
}

/// Returns the single `#[orm(primary_key)]` field, or a precise error.
pub(crate) fn require_primary_key<'a>(
    input: &DeriveInput,
    fields: &'a [FieldInfo<'a>],
) -> syn::Result<&'a FieldInfo<'a>> {
    let mut found = fields.iter().filter(|field| field.attrs.primary_key);
    let Some(primary) = found.next() else {
        return Err(Error::new(
            input.ident.span(),
            "missing primary key: mark exactly one field with #[orm(primary_key)]",
        ));
    };
    if found.next().is_some() {
        return Err(Error::new(
            input.ident.span(),
            "composite primary keys are not supported yet (phase 3 supports one key column)",
        ));
    }
    Ok(primary)
}

fn collect(fields: &syn::FieldsNamed) -> syn::Result<Vec<FieldInfo<'_>>> {
    fields
        .named
        .iter()
        .map(|field| {
            let attrs = parse_field(&field.attrs)?;
            let ident = field.ident.as_ref().expect("named field");
            let name = attrs.column.clone().unwrap_or_else(|| ident.to_string());
            Ok(FieldInfo {
                ident,
                ty: &field.ty,
                name,
                attrs,
            })
        })
        .collect()
}

fn lit_str(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<String> {
    Ok(meta.value()?.parse::<LitStr>()?.value())
}

fn attrs_span(attrs: &[Attribute]) -> proc_macro2::Span {
    attrs
        .first()
        .map(Attribute::path)
        .and_then(|path| path.segments.first())
        .map(|segment| segment.ident.span())
        .unwrap_or_else(proc_macro2::Span::call_site)
}
