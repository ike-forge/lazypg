//! A connection `url`: libpq's `postgresql://` form, or a JDBC URL copied from
//! an application's config.
//!
//! `tokio-postgres` parses the authority and path. The query string is ours:
//! its parser rejects JDBC-only parameters and knows no `verify-*` sslmode.

use std::num::NonZeroU16;
use std::str::FromStr;

use tokio_postgres::config::Host;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Url {
    pub(crate) host: Option<String>,
    pub(crate) port: Option<NonZeroU16>,
    pub(crate) database: Option<String>,
    pub(crate) user: Option<String>,
    pub(crate) password: Option<String>,
    pub(crate) params: Params,
    /// Parameter names, for a warning: a JDBC URL carries driver settings
    /// that mean nothing here.
    pub(crate) ignored: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Params {
    pub(crate) sslmode: Option<String>,
    pub(crate) sslrootcert: Option<String>,
    pub(crate) sslcert: Option<String>,
    pub(crate) sslkey: Option<String>,
}

pub(crate) fn parse(text: &str) -> Result<Url, String> {
    let text = text.strip_prefix("jdbc:").unwrap_or(text);
    if !(text.starts_with("postgresql://") || text.starts_with("postgres://")) {
        return Err("expected postgresql://, postgres:// or jdbc:postgresql://".to_owned());
    }
    let (base, query) = text.split_once('?').unwrap_or((text, ""));

    let parsed = tokio_postgres::Config::from_str(base).map_err(|error| error.to_string())?;
    let host = match parsed.get_hosts() {
        [] => None,
        [Host::Tcp(host)] => Some(host.clone()),
        [Host::Unix(path)] => Some(path.display().to_string()),
        _ => return Err("one host only".to_owned()),
    };
    // `tokio-postgres` fills in 5432 for a host without one; unstated here
    // leaves it to PGPORT.
    let port = match parsed.get_ports() {
        _ if !states_port(base) => None,
        [] => None,
        [port] => Some(NonZeroU16::new(*port).ok_or("port 0 is not a port")?),
        _ => return Err("one port only".to_owned()),
    };
    let password = parsed
        .get_password()
        .map(|bytes| String::from_utf8(bytes.to_vec()).map_err(|_| "password is not UTF-8"))
        .transpose()?;

    let mut url = Url {
        host,
        port,
        database: parsed.get_dbname().map(str::to_owned),
        user: parsed.get_user().map(str::to_owned),
        password,
        ..Url::default()
    };

    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = Some(decode(value)?);
        match key {
            "user" => url.user = value,
            "password" => url.password = value,
            "dbname" => url.database = value,
            "sslmode" => url.params.sslmode = value,
            "sslrootcert" => url.params.sslrootcert = value,
            "sslcert" => url.params.sslcert = value,
            "sslkey" => url.params.sslkey = value,
            other => url.ignored.push(other.to_owned()),
        }
    }

    Ok(url)
}

fn states_port(base: &str) -> bool {
    let authority = base
        .split_once("://")
        .map_or(base, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.rsplit_once(']').map_or(host, |(_, after)| after);
    host.contains(':')
}

fn decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%' {
            let digits = text
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                .ok_or_else(|| format!("`{text}`: malformed percent-escape"))?;
            out.push(digits);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| format!("`{text}` is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_libpq_url_fills_every_field_it_names() {
        let url =
            parse("postgresql://ana:s%40cret@db.internal:6543/orders?sslmode=verify-full").unwrap();

        assert_eq!(url.host.as_deref(), Some("db.internal"));
        assert_eq!(url.port.map(NonZeroU16::get), Some(6543));
        assert_eq!(url.database.as_deref(), Some("orders"));
        assert_eq!(url.user.as_deref(), Some("ana"));
        assert_eq!(url.password.as_deref(), Some("s@cret"));
        assert_eq!(url.params.sslmode.as_deref(), Some("verify-full"));
        assert!(url.ignored.is_empty());
    }

    #[test]
    fn a_jdbc_url_reads_its_query_and_names_what_it_ignores() {
        let url = parse(
            "jdbc:postgresql://db:5432/app?user=svc&password=p%26w&ssl=true&ApplicationName=x",
        )
        .unwrap();

        assert_eq!(url.user.as_deref(), Some("svc"));
        assert_eq!(url.password.as_deref(), Some("p&w"));
        assert_eq!(url.database.as_deref(), Some("app"));
        assert_eq!(url.ignored, ["ssl", "ApplicationName"]);
    }

    #[test]
    fn what_a_url_leaves_out_stays_unstated() {
        let url = parse("postgres://localhost").unwrap();

        assert_eq!(url.host.as_deref(), Some("localhost"));
        assert_eq!((url.port, url.database, url.user), (None, None, None));
        assert_eq!(
            parse("postgresql://u:p@[::1]:6543")
                .unwrap()
                .port
                .map(NonZeroU16::get),
            Some(6543)
        );
        assert_eq!(parse("postgresql://u:p@[::1]/db").unwrap().port, None);
    }

    #[test]
    fn a_url_is_refused_for_another_scheme_or_several_hosts() {
        assert!(parse("mysql://db/app").is_err());
        assert!(parse("postgresql://a:5432,b:5432/app").is_err());
        assert!(parse("postgresql://db/app?password=%zz").is_err());
    }
}
