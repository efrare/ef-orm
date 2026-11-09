//! Pure domain layer of **ef-ORM**.
//!
//! This crate holds the concepts every other crate builds on:
//!
//! - [`Value`] — a portable, driver-agnostic SQL value
//! - [`OrmError`] / [`Result`] — domain-level errors
//! - [`Entity`], [`Model`], [`Column`], [`PrimaryKey`] — static entity
//!   metadata implemented by `#[derive(Entity)]`/`#[derive(Model)]`
//!
//! It depends on **nothing but `thiserror`**: no SQL, no driver, no async
//! runtime. See `.opencode/skills/ef-orm/references/architecture.md` for the
//! dependency rule.

mod entity;
mod error;
mod value;

pub use entity::{Column, Entity, Model, PrimaryKey};
pub use error::{OrmError, Result};
pub use value::Value;
