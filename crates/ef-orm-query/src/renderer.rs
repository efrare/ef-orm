//! Shared render helpers: SQL text buffer + bound parameters.

use ef_orm_core::Value;

use crate::dialect::Dialect;
use crate::expr::Expr;
use crate::ident::{Ident, Table};

/// Accumulates SQL text and bound parameters while walking an AST.
///
/// Generic parts (identifiers, `WHERE`/boolean structure, comparisons) live
/// here; engine-specific layout (pagination, `OUTPUT`, DDL) lives in each
/// dialect's modules.
pub(crate) struct Renderer<'a> {
    dialect: &'a dyn Dialect,
    sql: String,
    params: Vec<Value>,
}

impl<'a> Renderer<'a> {
    pub(crate) fn new(dialect: &'a dyn Dialect) -> Self {
        Self {
            dialect,
            sql: String::new(),
            params: Vec::new(),
        }
    }

    /// Pushes raw SQL text (keywords, punctuation).
    pub(crate) fn push(&mut self, text: &str) {
        self.sql.push_str(text);
    }

    /// Pushes a quoted identifier.
    pub(crate) fn ident(&mut self, ident: &Ident) {
        let quoted = self.dialect.quote_ident(ident.as_str());
        self.push(&quoted);
    }

    /// Pushes a schema-qualified table (applying the dialect default schema).
    pub(crate) fn table(&mut self, table: &Table) {
        let dialect = self.dialect;
        let schema = table.schema.as_deref().or_else(|| dialect.default_schema());
        if let Some(schema) = schema {
            let quoted = dialect.quote_ident(schema);
            self.push(&quoted);
            self.push(".");
        }
        let name = dialect.quote_ident(&table.name);
        self.push(&name);
    }

    /// Binds `value` as the next parameter and pushes its placeholder.
    pub(crate) fn param(&mut self, value: Value) {
        self.params.push(value);
        let placeholder = self.dialect.placeholder(self.params.len());
        self.push(&placeholder);
    }

    /// Renders an expression in place.
    pub(crate) fn expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Column(ident) => self.ident(ident),
            Expr::Value(value) => self.param(value.clone()),
            Expr::Compare(lhs, op, rhs) => {
                self.expr(lhs);
                self.push(" ");
                self.push(op.as_str());
                self.push(" ");
                self.expr(rhs);
            }
            Expr::And(items) => self.logical(items, " AND ", true),
            Expr::Or(items) => self.logical(items, " OR ", false),
            Expr::Not(inner) => {
                self.push("NOT (");
                self.expr(inner);
                self.push(")");
            }
            Expr::IsNull { expr, negated } => {
                self.expr(expr);
                self.push(if *negated { " IS NOT NULL" } else { " IS NULL" });
            }
            Expr::In {
                expr,
                values,
                negated,
            } => self.in_list(expr, values, *negated),
        }
    }

    /// Consumes the renderer, returning `(sql, params)`.
    pub(crate) fn finish(self) -> (String, Vec<Value>) {
        (self.sql, self.params)
    }

    /// Joins items with `sep`; children needing grouping (an `OR` inside an
    /// `AND`, or vice versa) get parentheses. Empty `AND` ⇒ `1 = 1`,
    /// empty `OR` ⇒ `1 = 0`.
    fn logical(&mut self, items: &[Expr], sep: &str, is_and: bool) {
        if items.is_empty() {
            self.push(if is_and { "1 = 1" } else { "1 = 0" });
            return;
        }
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.push(sep);
            }
            let needs_parens =
                matches!((is_and, item), (true, Expr::Or(_)) | (false, Expr::And(_)));
            if needs_parens {
                self.push("(");
            }
            self.expr(item);
            if needs_parens {
                self.push(")");
            }
        }
    }

    fn in_list(&mut self, expr: &Expr, values: &[Value], negated: bool) {
        // T-SQL forbids `IN ()`; an empty list is a constant.
        if values.is_empty() {
            self.push(if negated { "1 = 1" } else { "1 = 0" });
            return;
        }
        self.expr(expr);
        self.push(if negated { " NOT IN (" } else { " IN (" });
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.param(value.clone());
        }
        self.push(")");
    }
}
