//! Proc-macros of **ef-ORM** (phase 3).
//!
//! Generate from a user struct:
//!
//! - `#[derive(Entity)]` — a `{Struct}Columns` struct of typed
//!   `ef_orm::ColumnRef`s plus the `ef_orm::Entity` metadata impl
//!   (table, columns, primary key)
//! - `#[derive(Model)]` — `ef_orm::FromRow` (row → struct, columns read
//!   **by name**) and `ef_orm::Model` (`to_values`, `id`, `set_id`)
//!
//! Attributes (shared by both derives):
//!
//! ```text
//! #[orm(table = "users", schema = "dbo")]  // struct level — `table` required by Entity
//! #[orm(primary_key, auto, column = "id")] // field level — one key field, `auto` = IDENTITY
//! ```
//!
//! Generated code only calls the public API of `ef-orm`, which the target
//! crate must depend on. Limitation: nullable fields must be spelled
//! `Option<T>` directly (type aliases are not recognized as nullable).

mod attrs;
mod entity;
mod model;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Derives the `ef_orm::Entity` metadata for a struct.
#[proc_macro_derive(Entity, attributes(orm))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    entity::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `ef_orm::Model` + `ef_orm::FromRow` for a struct.
#[proc_macro_derive(Model, attributes(orm))]
pub fn derive_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    model::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
