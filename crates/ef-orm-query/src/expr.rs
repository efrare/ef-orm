//! Boolean and value expressions used by columns, `WHERE` and `ORDER BY`.

use ef_orm_core::Value;

use crate::ident::Ident;

/// Comparison operators (rendered by the dialect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Like,
    NotLike,
}

impl CompareOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "<>",
            Self::Lt => "<",
            Self::LtEq => "<=",
            Self::Gt => ">",
            Self::GtEq => ">=",
            Self::Like => "LIKE",
            Self::NotLike => "NOT LIKE",
        }
    }
}

/// An expression usable in SELECT columns, WHERE, ORDER BY…
///
/// Values never appear inline: every [`Value`] becomes a bound parameter
/// (`@P1`, `@P2`, … in T-SQL). A `&str`/number converts to a **literal
/// value**; use [`Expr::col`] when you mean a column.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Column(Ident),
    Value(Value),
    Compare(Box<Expr>, CompareOp, Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    IsNull {
        expr: Box<Expr>,
        negated: bool,
    },
    In {
        expr: Box<Expr>,
        values: Vec<Value>,
        negated: bool,
    },
}

impl Expr {
    /// A column reference (`Expr::col("age")`).
    pub fn col(name: impl Into<Ident>) -> Self {
        Self::Column(name.into())
    }

    /// A bound value (`Expr::val(18)`, `Expr::val("A%")`).
    pub fn val(value: impl Into<Value>) -> Self {
        Self::Value(value.into())
    }

    pub fn and(self, rhs: impl Into<Expr>) -> Self {
        Self::And(vec![self, rhs.into()])
    }

    pub fn or(self, rhs: impl Into<Expr>) -> Self {
        Self::Or(vec![self, rhs.into()])
    }

    /// Groups all expressions with `AND` (empty ⇒ always true).
    pub fn all(items: impl IntoIterator<Item = Expr>) -> Self {
        Self::And(items.into_iter().collect())
    }

    /// Groups all expressions with `OR` (empty ⇒ always false).
    pub fn any(items: impl IntoIterator<Item = Expr>) -> Self {
        Self::Or(items.into_iter().collect())
    }

    pub fn is_null(self) -> Self {
        Self::IsNull {
            expr: Box::new(self),
            negated: false,
        }
    }

    pub fn is_not_null(self) -> Self {
        Self::IsNull {
            expr: Box::new(self),
            negated: true,
        }
    }

    pub fn is_in<V: Into<Value>>(self, values: impl IntoIterator<Item = V>) -> Self {
        Self::In {
            expr: Box::new(self),
            values: values.into_iter().map(Into::into).collect(),
            negated: false,
        }
    }

    pub fn is_not_in<V: Into<Value>>(self, values: impl IntoIterator<Item = V>) -> Self {
        Self::In {
            expr: Box::new(self),
            values: values.into_iter().map(Into::into).collect(),
            negated: true,
        }
    }

    pub fn eq(self, rhs: impl Into<Expr>) -> Self {
        self.compare(CompareOp::Eq, rhs)
    }

    pub fn ne(self, rhs: impl Into<Expr>) -> Self {
        self.compare(CompareOp::NotEq, rhs)
    }

    pub fn lt(self, rhs: impl Into<Expr>) -> Self {
        self.compare(CompareOp::Lt, rhs)
    }

    pub fn le(self, rhs: impl Into<Expr>) -> Self {
        self.compare(CompareOp::LtEq, rhs)
    }

    pub fn gt(self, rhs: impl Into<Expr>) -> Self {
        self.compare(CompareOp::Gt, rhs)
    }

    pub fn ge(self, rhs: impl Into<Expr>) -> Self {
        self.compare(CompareOp::GtEq, rhs)
    }

    pub fn like(self, rhs: impl Into<Expr>) -> Self {
        self.compare(CompareOp::Like, rhs)
    }

    pub fn not_like(self, rhs: impl Into<Expr>) -> Self {
        self.compare(CompareOp::NotLike, rhs)
    }

    pub fn compare(self, op: CompareOp, rhs: impl Into<Expr>) -> Self {
        Self::Compare(Box::new(self), op, Box::new(rhs.into()))
    }
}

/// Accumulates `AND`-chained filters: `None → new`, `Some(prev) → prev AND new`.
pub(crate) fn combine_filter(slot: &mut Option<Expr>, new: Expr) {
    *slot = Some(match slot.take() {
        Some(prev) => Expr::And(vec![prev, new]),
        None => new,
    });
}

macro_rules! from_value_expr {
    ($($t:ty),* $(,)?) => {
        $(
            impl From<$t> for Expr {
                fn from(v: $t) -> Self {
                    Self::Value(v.into())
                }
            }
        )*
    };
}

from_value_expr!(
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    bool,
    f32,
    f64,
    String,
    Vec<u8>
);

impl core::ops::Not for Expr {
    type Output = Expr;

    /// Logical negation: `!Expr::col("a").eq(1)` ⇒ `NOT ([a] = @P1)`.
    fn not(self) -> Self::Output {
        Self::Not(Box::new(self))
    }
}

impl From<&str> for Expr {
    fn from(v: &str) -> Self {
        Self::Value(v.into())
    }
}

impl From<Value> for Expr {
    fn from(v: Value) -> Self {
        Self::Value(v)
    }
}

impl From<Ident> for Expr {
    fn from(v: Ident) -> Self {
        Self::Column(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_chain_with_and() {
        let mut filter = None;
        combine_filter(&mut filter, Expr::col("a").eq(1));
        combine_filter(&mut filter, Expr::col("b").eq(2));
        assert_eq!(
            filter,
            Some(Expr::And(vec![Expr::col("a").eq(1), Expr::col("b").eq(2)]))
        );
    }

    #[test]
    fn empty_in_holds_no_values() {
        let expr = Expr::col("id").is_in(Vec::<i32>::new());
        assert!(matches!(expr, Expr::In { values, negated: false, .. } if values.is_empty()));
    }
}
