//! Code generation for `#[derive(Model)]` — row mapping and identity.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, LitStr};

use crate::attrs::{self, FieldInfo};

/// Emits the `FromRow` (read) and `Model` (write/identity) impls.
pub(crate) fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &input.ident;
    // Parsed for validation even when only `Model` is derived (the attribute
    // must be consumed by some derive on the item).
    attrs::parse_struct(&input.attrs)?;
    let fields = attrs::field_infos(input)?;
    if fields.is_empty() {
        return Err(syn::Error::new(
            name.span(),
            "#[derive(Model)] needs at least one column field",
        ));
    }
    let primary = attrs::require_primary_key(input, &fields)?;

    let from_row = from_row_fields(&fields);
    let inserts = insert_pushes(&fields);
    let capacity = fields.len();
    let primary_ident = primary.ident;

    Ok(quote! {
        impl ::ef_orm::FromRow for #name {
            fn from_row<R: ::ef_orm::Row>(
                row: &R,
            ) -> ::std::result::Result<Self, ::ef_orm::OrmError> {
                ::std::result::Result::Ok(Self {
                    #(#from_row)*
                })
            }
        }

        impl ::ef_orm::Model for #name {
            fn to_values(&self) -> ::std::vec::Vec<(&'static str, ::ef_orm::Value)> {
                let mut values = ::std::vec::Vec::with_capacity(#capacity);
                #(#inserts)*
                values
            }

            fn id(&self) -> Self::Id {
                ::std::clone::Clone::clone(&self.#primary_ident)
            }

            fn set_id(
                &mut self,
                id: ::ef_orm::Value,
            ) -> ::std::result::Result<(), ::ef_orm::OrmError> {
                self.#primary_ident = ::ef_orm::FromValue::from_value(&id)?;
                ::std::result::Result::Ok(())
            }
        }
    })
}

/// One struct-literal entry per field: `Option` fields accept NULL, others
/// must have a value (column names are read **by name**, never by index).
fn from_row_fields(fields: &[FieldInfo<'_>]) -> Vec<TokenStream> {
    fields
        .iter()
        .map(|field| {
            let ident = field.ident;
            let column = LitStr::new(&field.name, field.ident.span());
            if field.is_option() {
                quote!(#ident: row.get_by_name(#column)?,)
            } else {
                quote! {
                    #ident: row.get_by_name(#column)?.ok_or_else(|| {
                        ::ef_orm::OrmError::Conversion(
                            ::std::format!("column '{}' is NULL but the field is not Option", #column),
                        )
                    })?,
                }
            }
        })
        .collect()
}

/// One `values.push((…))` per persisted field — the auto-increment key is
/// excluded because the server generates it.
fn insert_pushes(fields: &[FieldInfo<'_>]) -> Vec<TokenStream> {
    fields
        .iter()
        .filter(|field| !(field.attrs.primary_key && field.attrs.auto))
        .map(|field| {
            let ident = field.ident;
            let column = LitStr::new(&field.name, field.ident.span());
            quote! {
                values.push((
                    #column,
                    ::ef_orm::Value::from(::std::clone::Clone::clone(&self.#ident)),
                ));
            }
        })
        .collect()
}
