//! Connection settings for the TDS driver.
//!
//! Credentials are never printed: [`Debug`] is hand-written and redacts the
//! password (pattern borrowed from `sql-orm-main`).

use std::fmt;
use tiberius::{AuthMethod, Config as TiberiusConfig};

/// How to reach a SQL Server instance: address, database, login and TLS.
#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    /// Hostname or IP of the server (no `tcp:` prefix).
    pub host: String,
    /// Server port (1433 is the SQL Server default).
    pub port: u16,
    /// Database to open the session in (e.g. `master`).
    pub database: String,
    /// SQL Server login user.
    pub user: String,
    /// SQL Server login password — redacted in `Debug`.
    pub password: String,
    /// Accept the server certificate without validating it against the
    /// system trust store. **Development only**: self-signed certificates
    /// are common on local servers; keep `false` in production.
    pub trust_cert: bool,
}

impl Config {
    /// Builds a config with TLS certificate validation enabled
    /// ([`Config::trust_cert`] opts out, for dev servers).
    pub fn new(
        host: impl Into<String>,
        port: u16,
        database: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            host: host.into(),
            port,
            database: database.into(),
            user: user.into(),
            password: password.into(),
            trust_cert: false,
        }
    }

    /// `host:port` — the address used for the TCP dial.
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Converts into the tiberius connection settings.
    pub(crate) fn to_tiberius(&self) -> TiberiusConfig {
        let mut builder = TiberiusConfig::builder()
            .host(self.host.clone())
            .port(self.port)
            .database(self.database.clone())
            .authentication(AuthMethod::sql_server(
                self.user.clone(),
                self.password.clone(),
            ));
        if self.trust_cert {
            builder = builder.trust_cert();
        }
        builder.build()
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("addr", &self.addr())
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &"<redacted>")
            .field("trust_cert", &self.trust_cert)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    fn dev_config() -> Config {
        Config::new("192.168.0.10", 1433, "master", "sa", "S3cret!")
    }

    #[test]
    fn addr_joins_host_and_port() {
        assert_eq!(dev_config().addr(), "192.168.0.10:1433");
    }

    #[test]
    fn debug_redacts_the_password() {
        let debug = format!("{:?}", dev_config());

        assert!(debug.contains("Config"));
        assert!(debug.contains(r#"password: "<redacted>""#));
        assert!(debug.contains(r#"addr: "192.168.0.10:1433""#));
        assert!(!debug.contains("S3cret!"));
    }

    #[test]
    fn builds_a_tiberius_config_with_the_same_address() {
        let config = dev_config().to_tiberius();

        assert_eq!(config.get_addr(), "192.168.0.10:1433");
    }

    #[test]
    fn certificate_validation_is_on_by_default() {
        assert!(!dev_config().trust_cert);

        let mut dev = dev_config();
        dev.trust_cert = true;
        assert!(dev.trust_cert);
    }
}
