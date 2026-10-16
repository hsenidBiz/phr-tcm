//! Parsing the application's ADO/OLE DB connection strings into a driver config.
//!
//! The keyword set and aliases mirror the application's own parser (`db/sqlcmd.rs` in the
//! consumer app): keys fold case- and space-insensitively, `Server|Data Source|Address|Addr`
//! name the host (an optional `,port` suffix is the SQL Server client convention),
//! `Database|Initial Catalog` the database, `User Id|UID|User` and `Password|PWD` the SQL
//! login, `TrustServerCertificate` accepts `true`/`yes`/`1`. A missing key is an error naming
//! ONLY that key — the rest of the string is a credential.

use std::fmt;

use tiberius::AuthMethod;

/// A parsed SQL Server connection. `Debug` hides the password.
#[derive(Clone)]
pub struct ConnectionConfig {
    /// Host (server name or address).
    pub server: String,
    /// TCP port; SQL Server's default 1433 when the string carries none.
    pub port: u16,
    /// Initial catalog / database name.
    pub database: String,
    /// SQL auth user.
    pub user: String,
    /// SQL auth password — never rendered by `Debug`.
    pub password: String,
    /// Accept any server certificate during TLS (ADO `TrustServerCertificate`).
    pub trust_cert: bool,
}

impl fmt::Debug for ConnectionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionConfig")
            .field("server", &self.server)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &"(hidden)")
            .field("trust_cert", &self.trust_cert)
            .finish()
    }
}

impl ConnectionConfig {
    /// The `server/database` label used in logs and error messages — identifies the target
    /// without the credential.
    pub fn label(&self) -> String {
        format!("{}/{}", self.server, self.database)
    }

    /// Build the tiberius auth + endpoint config. TLS: login is always encrypted by the TDS
    /// protocol; `trust_cert` (the ADO string's `TrustServerCertificate=True`) disables
    /// certificate validation for servers with self-signed certs.
    pub fn tiberius(&self) -> tiberius::Config {
        let mut cfg = tiberius::Config::new();
        cfg.host(&self.server);
        cfg.port(self.port);
        cfg.database(&self.database);
        if self.trust_cert {
            cfg.trust_cert();
        }
        cfg.authentication(AuthMethod::sql_server(&self.user, &self.password));
        cfg
    }
}

fn key_of(part: &str) -> Option<(String, &str)> {
    let eq = part.find('=')?;
    let key = part[..eq].trim().to_ascii_lowercase();
    let value = part[eq + 1..].trim();
    Some((key, value))
}

fn truthy(v: &str) -> bool {
    matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "1")
}

/// Parse an ADO/OLE DB connection string. Missing required keys are errors naming only the
/// key, never the surrounding credential material.
///
/// # Errors
///
/// - no `Server`/`Data Source` key
/// - no `Database`/`Initial Catalog` key
/// - no `User Id` key
/// - no `Password` key
/// - a `,port` suffix on the server that does not parse as a port number
pub fn parse_connection_string(s: &str) -> Result<ConnectionConfig, String> {
    let mut server: Option<(String, u16)> = None;
    let mut database = None;
    let mut user = None;
    let mut password = None;
    let mut trust_cert = false;

    for part in s.split(';') {
        let Some((key, value)) = key_of(part) else { continue };
        match key.as_str() {
            "server" | "data source" | "datasource" | "address" | "addr" | "network address" => {
                let (host, port) = match value.split_once(',') {
                    Some((h, p)) => (
                        h.trim().to_string(),
                        p.trim().parse::<u16>().map_err(|_| {
                            format!("the connection string's Server port `{p}` is not a port number")
                        })?,
                    ),
                    None => (value.to_string(), 1433),
                };
                server = Some((host, port));
            }
            "database" | "initial catalog" | "initialcatalog" => database = Some(value.to_string()),
            "user id" | "userid" | "uid" | "user" => user = Some(value.to_string()),
            "password" | "pwd" => password = Some(value.to_string()),
            "trustservercertificate" => trust_cert = truthy(value),
            _ => {}
        }
    }

    let (server, port) = server.ok_or("the connection string has no Server=")?;
    Ok(ConnectionConfig {
        server,
        port,
        database: database.ok_or("the connection string has no Database=")?,
        user: user.ok_or("the connection string has no User Id=")?,
        password: password.ok_or("the connection string has no Password=")?,
        trust_cert,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "Server=10.56.3.16;Database=hrmmain_philippinesdev;User Id=rpt;Password=hush;TrustServerCertificate=True";

    #[test]
    fn parses_the_application_shape() {
        let c = parse_connection_string(FULL).unwrap();
        assert_eq!(c.server, "10.56.3.16");
        assert_eq!(c.port, 1433);
        assert_eq!(c.database, "hrmmain_philippinesdev");
        assert_eq!(c.user, "rpt");
        assert_eq!(c.password, "hush");
        assert!(c.trust_cert);
        assert_eq!(c.label(), "10.56.3.16/hrmmain_philippinesdev");
    }

    #[test]
    fn takes_the_ado_aliases_and_comma_port() {
        let c = parse_connection_string(
            "Data Source=host.cloud,1443;Initial Catalog=db;UID=me;PWD=p;TrustServerCertificate=yes",
        )
        .unwrap();
        assert_eq!((c.server.as_str(), c.port, c.database.as_str(), c.user.as_str()), ("host.cloud", 1443, "db", "me"));
        assert!(c.trust_cert);
    }

    #[test]
    fn missing_keys_error_naming_only_the_key() {
        // Distinctive secret values: an error echoing any of them is a leak.
        for (s, want) in [
            ("Database=d;User Id=u;Password=sekrit1", "Server"),
            ("Server=s;User Id=u;Password=sekrit2", "Database"),
            ("Server=s;Database=d;Password=sekrit3", "User Id"),
            ("Server=s;Database=d;User Id=u", "Password"),
        ] {
            let e = parse_connection_string(s).unwrap_err();
            assert!(e.contains(want), "`{e}` should name {want}");
            for leaked in ["sekrit1", "sekrit2", "sekrit3"] {
                assert!(!e.contains(leaked), "error leaks the credential: {e}");
            }
        }
    }

    #[test]
    fn debug_and_label_never_expose_the_password() {
        let c = parse_connection_string(FULL).unwrap();
        let d = format!("{c:?}");
        assert!(!d.contains("hush"), "Debug leaks: {d}");
        assert!(d.contains("(hidden)"));
        assert!(!c.label().contains("hush"));
    }

    #[test]
    fn bad_port_is_named_without_the_rest() {
        let e = parse_connection_string("Server=host,notaport;Database=d;User Id=u;Password=p")
            .unwrap_err();
        assert!(e.contains("notaport"));
        assert!(!e.contains("Password"));
    }
}
