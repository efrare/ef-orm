//! Typed column references for the fluent `filter`/`order_by` API.
//!
//! `#[derive(Entity)]` generates a `{Struct}Columns` struct whose fields are
//! [`ColumnRef`]s over each field's Rust type, so
//! `select().filter(|c| c.age.gt(18))` references columns by name (no stringly
//! typos) and `c.age.like("%x")` only compiles for text columns.

use ef_orm_core::{Column, Value};

use crate::expr::{CompareOp, Expr};

/// A reference to one column of an entity, typed by `T` — the Rust type the
/// column maps to (e.g. `ColumnRef<i32>` for an `INT` column).
///
/// Construction happens through the generated `{Struct}Columns` struct; the
/// name is `'static` because it comes from entity metadata.
pub struct ColumnRef<T> {
    name: &'static str,
    _ty: std::marker::PhantomData<fn() -> T>,
}

// Manual impls: `#[derive]` would wrongly demand `T: Copy`/`T: Clone`.
impl<T> Clone for ColumnRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ColumnRef<T> {}

impl<T> ColumnRef<T> {
    /// A column reference with the given (unquoted) SQL name.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            _ty: std::marker::PhantomData,
        }
    }
}

impl<T> Column for ColumnRef<T> {
    fn name(&self) -> &'static str {
        self.name
    }
}

/// Comparisons and NULL tests — available for every column type.
impl<T> ColumnRef<T> {
    fn compare(self, op: CompareOp, rhs: impl Into<Value>) -> Expr {
        Expr::col(self.name).compare(op, Expr::val(rhs.into()))
    }

    /// `column = value`. Comparing against `Value::Null` becomes
    /// [`ColumnRef::is_null`] (SQL `= NULL` never matches).
    pub fn eq(self, rhs: impl Into<Value>) -> Expr {
        match rhs.into() {
            Value::Null => self.is_null(),
            value => self.compare(CompareOp::Eq, value),
        }
    }

    /// `column <> value`. Comparing against `Value::Null` becomes
    /// [`ColumnRef::is_not_null`] (SQL `<> NULL` never matches).
    pub fn ne(self, rhs: impl Into<Value>) -> Expr {
        match rhs.into() {
            Value::Null => self.is_not_null(),
            value => self.compare(CompareOp::NotEq, value),
        }
    }

    pub fn lt(self, rhs: impl Into<Value>) -> Expr {
        self.compare(CompareOp::Lt, rhs)
    }

    pub fn le(self, rhs: impl Into<Value>) -> Expr {
        self.compare(CompareOp::LtEq, rhs)
    }

    pub fn gt(self, rhs: impl Into<Value>) -> Expr {
        self.compare(CompareOp::Gt, rhs)
    }

    pub fn ge(self, rhs: impl Into<Value>) -> Expr {
        self.compare(CompareOp::GtEq, rhs)
    }

    /// `column IS NULL`.
    pub fn is_null(self) -> Expr {
        Expr::col(self.name).is_null()
    }

    /// `column IS NOT NULL`.
    pub fn is_not_null(self) -> Expr {
        Expr::col(self.name).is_not_null()
    }

    /// `column IN (…)`. An empty list renders `1 = 0` (never true).
    pub fn is_in<V: Into<Value>>(self, values: impl IntoIterator<Item = V>) -> Expr {
        Expr::col(self.name).is_in(values)
    }

    /// `column NOT IN (…)`. An empty list renders `1 = 1` (always true).
    pub fn is_not_in<V: Into<Value>>(self, values: impl IntoIterator<Item = V>) -> Expr {
        Expr::col(self.name).is_not_in(values)
    }
}

/// Marker for columns whose Rust type is text — only they accept `LIKE`.
pub trait TextColumn {}

impl TextColumn for String {}
impl TextColumn for Option<String> {}

/// `LIKE` / `NOT LIKE`, restricted to text columns ([`TextColumn`]).
impl<T: TextColumn> ColumnRef<T> {
    pub fn like(self, rhs: impl Into<Value>) -> Expr {
        self.compare(CompareOp::Like, rhs)
    }

    pub fn not_like(self, rhs: impl Into<Value>) -> Expr {
        self.compare(CompareOp::NotLike, rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::{Column, ColumnRef};
    use crate::expr::{CompareOp, Expr};
    use ef_orm_core::Value;

    #[test]
    fn builds_comparisons_bound_as_parameters() {
        let age = ColumnRef::<i32>::new("age");

        assert_eq!(age.name(), "age");
        assert_eq!(age.gt(18), Expr::col("age").gt(Expr::val(18)));
        assert_eq!(
            age.le(30),
            Expr::Compare(
                Box::new(Expr::col("age")),
                CompareOp::LtEq,
                Box::new(Expr::val(30))
            )
        );
        assert_eq!(age.is_in([18, 20]), Expr::col("age").is_in([18, 20]));
    }

    #[test]
    fn null_comparisons_become_is_null_predicates() {
        let name = ColumnRef::<String>::new("name");

        assert_eq!(name.eq(Value::Null), Expr::col("name").is_null());
        assert_eq!(name.ne(Value::Null), Expr::col("name").is_not_null());
        assert_eq!(name.is_null(), Expr::col("name").is_null());
    }

    #[test]
    fn like_is_available_for_text_columns() {
        let name = ColumnRef::<String>::new("name");
        let nickname = ColumnRef::<Option<String>>::new("nickname");

        assert_eq!(name.like("A%"), Expr::col("name").like(Expr::val("A%")));
        assert_eq!(
            nickname.not_like("%x"),
            Expr::col("nickname").not_like(Expr::val("%x"))
        );
    }

    #[test]
    fn references_are_copy_and_share_nothing() {
        let age = ColumnRef::<i32>::new("age");
        let copy = age;
        assert_eq!(age.name(), copy.name());
    }
}
