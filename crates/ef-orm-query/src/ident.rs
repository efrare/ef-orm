//! Identifiers: columns and schema-qualified tables.

use core::fmt;

/// A SQL identifier (column, alias…). Quoting is the dialect's job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ident(String);

impl Ident {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The raw, unquoted name.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the identifier, returning the inner raw name.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<&str> for Ident {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for Ident {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl fmt::Display for Ident {
    /// Writes the **raw** name (quoting happens in the dialect renderer).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A possibly schema-qualified table reference.
///
/// Without an explicit schema the dialect's default applies (T-SQL: `dbo`),
/// so rendered SQL never depends on a session's default schema.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Table {
    pub schema: Option<String>,
    pub name: String,
}

impl Table {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema: None,
            name: name.into(),
        }
    }

    pub fn with_schema(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema: Some(schema.into()),
            name: name.into(),
        }
    }
}

impl From<&str> for Table {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl From<String> for Table {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl From<Ident> for Table {
    fn from(name: Ident) -> Self {
        Self::new(name.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_without_schema_is_pending_dialect_default() {
        assert_eq!(Table::from("users").schema, None);
        assert_eq!(
            Table::with_schema("sales", "orders"),
            Table {
                schema: Some("sales".into()),
                name: "orders".into()
            }
        );
    }
}
