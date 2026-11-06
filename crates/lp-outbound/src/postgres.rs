//! The `Cluster` port over `tokio-postgres`.
//!
//! Rows are read with the simple query protocol, so every value arrives as
//! Postgres's own text rendering and no type needs a decoder of ours. Column
//! types come from a `prepare` of the same text; a multi-statement string
//! cannot be prepared and reads as all text.

use std::collections::HashMap;
use std::num::NonZeroU16;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lp_domain::config::{ConnectionConfig, DriverConfig, PageSize, SslMode};
use lp_domain::contents::TableRef;
use lp_domain::health::Health;
use lp_domain::model::{
    Catalog, ConnId, DbError, Page, PagingState, Row, Statement, StatementKind, Value,
};
use lp_domain::ports::{Cluster, SecretResolver as _};
use lp_domain::posture::Posture;
use tokio::sync::RwLock;
use tokio_postgres::config::SslMode as DriverSslMode;
use tokio_postgres::error::SqlState;
use tokio_postgres::types::Type;
use tokio_postgres::{CancelToken, Client, NoTls, SimpleQueryMessage};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::secret::{self, Secrets};
use crate::tls;

#[derive(Debug, Default)]
pub struct Postgres {
    open: RwLock<HashMap<ConnId, Arc<Open>>>,
}

struct Open {
    client: Client,
    cancel: CancelToken,
    tls: Option<MakeRustlsConnect>,
    request_timeout: Duration,
}

impl std::fmt::Debug for Open {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Open")
            .field("tls", &self.tls.is_some())
            .field("request_timeout", &self.request_timeout)
            .finish_non_exhaustive()
    }
}

impl Postgres {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    async fn session(&self, conn: ConnId) -> Result<Arc<Open>, DbError> {
        self.open
            .read()
            .await
            .get(&conn)
            .cloned()
            .ok_or_else(|| DbError::Unreachable("not connected".to_owned()))
    }
}

#[async_trait]
impl Cluster for Postgres {
    async fn connect(
        &self,
        conn: ConnId,
        target: &ConnectionConfig,
        driver: &DriverConfig,
    ) -> Result<Posture, DbError> {
        let resolved = Resolved::of(target, |key| std::env::var(key).ok());
        let mut config = tokio_postgres::Config::new();
        config
            .application_name("lazypg")
            .connect_timeout(millis(driver.connect_timeout.get()))
            .host(&resolved.host)
            .port(resolved.port)
            .user(&resolved.user);
        // Unset, the server takes the user's name, as `resolved.database` says.
        if let Some(database) = &target.database {
            config.dbname(database);
        } else if let Ok(database) = std::env::var("PGDATABASE") {
            config.dbname(database);
        }

        let password = match &target.password {
            // Resolved here so the plaintext never enters `lp-domain`.
            Some(credential) => Some(
                Secrets
                    .resolve(credential)
                    .await
                    .map_err(|error| DbError::Unreachable(error.to_string()))?,
            ),
            None => match std::env::var("PGPASSWORD") {
                Ok(password) => Some(password),
                Err(_) => secret::passfile(resolved.passfile_target()).await,
            },
        };
        if let Some(password) = password {
            config.password(password);
        }

        // The server-side rail (architecture.md §4): a statement the domain
        // misclassifies is still refused.
        if target.posture == Posture::ReadOnly {
            config.options("-c default_transaction_read_only=on");
        }

        let tls = match resolved.sslmode {
            SslMode::Disable => None,
            mode => {
                config.ssl_mode(if mode == SslMode::Prefer {
                    DriverSslMode::Prefer
                } else {
                    DriverSslMode::Require
                });
                Some(MakeRustlsConnect::new(
                    (*tls::client_config(mode, target).await?).clone(),
                ))
            }
        };

        let client = match &tls {
            None => {
                let (client, connection) = config
                    .connect(NoTls)
                    .await
                    .map_err(|error| connect_error(&error))?;
                tokio::spawn(connection);
                client
            }
            Some(tls) => {
                let (client, connection) = config
                    .connect(tls.clone())
                    .await
                    .map_err(|error| connect_error(&error))?;
                tokio::spawn(connection);
                client
            }
        };

        let open = Open {
            cancel: client.cancel_token(),
            client,
            tls,
            request_timeout: millis(driver.request_timeout.get()),
        };
        self.open.write().await.insert(conn, Arc::new(open));

        Ok(target.posture)
    }

    async fn describe(&self, conn: ConnId) -> Result<Catalog, DbError> {
        let open = self.session(conn).await?;
        crate::catalog::describe(&open.client).await
    }

    async fn execute(&self, conn: ConnId, stmt: &Statement) -> Result<Page, DbError> {
        let open = self.session(conn).await?;
        run(&open, &stmt.sql).await
    }

    /// `LIMIT/OFFSET` around the statement (architecture.md §6). One row past
    /// the page tells whether there is another.
    async fn execute_paged(
        &self,
        conn: ConnId,
        stmt: &Statement,
        page_size: PageSize,
        cursor: Option<&PagingState>,
    ) -> Result<Page, DbError> {
        let open = self.session(conn).await?;

        let Some(inner) = pageable(stmt) else {
            return run(&open, &stmt.sql).await;
        };

        let offset = cursor.map_or(0, offset_of);
        let size = u64::from(page_size.get());
        let sql = format!(
            "SELECT * FROM ({inner}) AS lazypg_page LIMIT {} OFFSET {offset}",
            size + 1
        );

        let mut page = run(&open, &sql).await?;
        if page.rows.len() as u64 > size {
            page.rows.truncate(page_size.get() as usize);
            page.next = Some(PagingState((offset + size).to_be_bytes().to_vec()));
        }
        Ok(page)
    }

    async fn table_health(&self, _conn: ConnId, _table: &TableRef) -> Result<Health, DbError> {
        Err(DbError::Other(
            "table health is not available for Postgres yet".to_owned(),
        ))
    }
}

/// libpq's resolution for every field but the password, which may read a file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Resolved {
    host: String,
    port: u16,
    user: String,
    /// What the server will use: unset, the user's name.
    database: String,
    sslmode: SslMode,
}

impl Resolved {
    fn of(target: &ConnectionConfig, env: impl Fn(&str) -> Option<String>) -> Self {
        let user = target
            .user
            .clone()
            .or_else(|| env("PGUSER"))
            .or_else(|| env("USER"))
            .unwrap_or_else(|| "postgres".to_owned());
        Self {
            host: target
                .host
                .clone()
                .or_else(|| env("PGHOST"))
                .unwrap_or_else(|| "localhost".to_owned()),
            port: target
                .port
                .map(NonZeroU16::get)
                .or_else(|| env("PGPORT").and_then(|port| port.parse().ok()))
                .unwrap_or(5432),
            database: target
                .database
                .clone()
                .or_else(|| env("PGDATABASE"))
                .unwrap_or_else(|| user.clone()),
            sslmode: target
                .sslmode
                .or_else(|| {
                    let word = env("PGSSLMODE")?;
                    SslMode::ALL.into_iter().find(|mode| mode.label() == word)
                })
                .unwrap_or_default(),
            user,
        }
    }

    fn passfile_target(&self) -> secret::Target<'_> {
        secret::Target {
            host: &self.host,
            port: self.port,
            database: &self.database,
            user: &self.user,
        }
    }
}

fn millis(value: u32) -> Duration {
    Duration::from_millis(value.into())
}

/// The statement without its trailing `;`, when wrapping it in a subquery is
/// valid; `None` runs it as written, unpaged.
fn pageable(stmt: &Statement) -> Option<&str> {
    if stmt.kind != StatementKind::Read {
        return None;
    }
    let inner = stmt.sql.trim().trim_end_matches(';').trim_end();
    let first = inner
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()?
        .to_ascii_uppercase();

    // A `;` left inside means several statements, or one in a literal; both
    // run unwrapped rather than risk a broken subquery.
    (matches!(first.as_str(), "SELECT" | "VALUES" | "TABLE" | "WITH") && !inner.contains(';'))
        .then_some(inner)
}

fn offset_of(PagingState(bytes): &PagingState) -> u64 {
    bytes.as_slice().try_into().map_or(0, u64::from_be_bytes)
}

/// Aborting the future that holds this — `Effect::Cancel` aborts the task —
/// sends a cancel request, so the server stops rather than finishing unseen.
struct CancelOnDrop {
    open: Option<Arc<Open>>,
}

impl CancelOnDrop {
    fn disarm(mut self) {
        self.open = None;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        runtime.spawn(async move {
            let token = open.cancel.clone();
            // Best effort: a failed cancel leaves the query to its own timeout.
            let _ = match &open.tls {
                Some(tls) => token.cancel_query(tls.clone()).await,
                None => token.cancel_query(NoTls).await,
            };
        });
    }
}

async fn run(open: &Arc<Open>, sql: &str) -> Result<Page, DbError> {
    let guard = CancelOnDrop {
        open: Some(Arc::clone(open)),
    };

    let read = async {
        let types: Option<Vec<Type>> = open.client.prepare(sql).await.ok().map(|prepared| {
            prepared
                .columns()
                .iter()
                .map(|c| c.type_().clone())
                .collect()
        });
        let messages = open
            .client
            .simple_query(sql)
            .await
            .map_err(|error| db_error(&error))?;
        Ok::<_, DbError>(page(messages, types.as_deref()))
    };

    // An elapsed timeout drops the guard armed, which cancels server-side.
    let page = tokio::time::timeout(open.request_timeout, read)
        .await
        .map_err(|_| DbError::Timeout)??;
    guard.disarm();
    Ok(page)
}

/// The last result set wins, as in psql.
fn page(messages: Vec<SimpleQueryMessage>, types: Option<&[Type]>) -> Page {
    let mut columns = Vec::new();
    let mut rows = Vec::new();

    for message in messages {
        match message {
            SimpleQueryMessage::RowDescription(described) => {
                columns = described.iter().map(|c| c.name().to_owned()).collect();
                rows.clear();
            }
            SimpleQueryMessage::Row(row) => {
                let values = (0..row.len())
                    .map(|i| {
                        let ty = types.and_then(|t| t.get(i));
                        value(row.get(i), ty)
                    })
                    .collect();
                rows.push(Row { values });
            }
            _ => {}
        }
    }

    Page {
        columns,
        rows,
        next: None,
    }
}

fn value(text: Option<&str>, ty: Option<&Type>) -> Value {
    let Some(text) = text else {
        return Value::Null;
    };
    let as_text = || Value::Text(text.to_owned());

    match ty {
        Some(&Type::BOOL) => Value::Boolean(text == "t"),
        Some(&Type::INT2 | &Type::INT4 | &Type::INT8 | &Type::OID) => {
            text.parse().map_or_else(|_| as_text(), Value::Int)
        }
        Some(&Type::FLOAT4 | &Type::FLOAT8) => {
            text.parse().map_or_else(|_| as_text(), Value::Float)
        }
        // Text: it is arbitrary precision.
        Some(&Type::NUMERIC) => Value::Numeric(text.to_owned()),
        Some(&Type::BYTEA) => hex(text).map_or_else(as_text, Value::Blob),
        Some(&Type::UUID) => Value::Uuid(text.to_owned()),
        _ => as_text(),
    }
}

/// `bytea_output = hex`, the default since 9.0.
fn hex(text: &str) -> Option<Vec<u8>> {
    let digits = text.strip_prefix("\\x")?;
    if digits.len() % 2 != 0 {
        return None;
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(digits.get(i..i + 2)?, 16).ok())
        .collect()
}

fn connect_error(error: &tokio_postgres::Error) -> DbError {
    match error.as_db_error() {
        Some(db)
            if [
                SqlState::INVALID_PASSWORD,
                SqlState::INVALID_AUTHORIZATION_SPECIFICATION,
            ]
            .contains(db.code()) =>
        {
            DbError::Authentication(db.message().to_owned())
        }
        Some(_) => db_error(error),
        None => DbError::Unreachable(error.to_string()),
    }
}

pub(crate) fn db_error(error: &tokio_postgres::Error) -> DbError {
    let Some(db) = error.as_db_error() else {
        return if error.is_closed() {
            DbError::Unreachable(error.to_string())
        } else {
            DbError::Other(error.to_string())
        };
    };

    match *db.code() {
        SqlState::READ_ONLY_SQL_TRANSACTION => DbError::ReadOnly,
        SqlState::QUERY_CANCELED => DbError::Cancelled,
        _ => DbError::Invalid(match db.hint() {
            Some(hint) => format!("{} (hint: {hint})", db.message()),
            None => db.message().to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use lp_domain::config::ConnectionName;

    use super::*;

    fn read(sql: &str) -> Statement {
        Statement {
            sql: sql.to_owned(),
            kind: StatementKind::Read,
        }
    }

    fn env(vars: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        |key| {
            vars.iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    fn named() -> ConnectionConfig {
        ConnectionConfig::named(ConnectionName::try_new("t").unwrap())
    }

    #[test]
    fn an_unstated_field_falls_back_to_its_pg_variable_then_libpq_default() {
        let bare = Resolved::of(&named(), env(&[("USER", "ana")]));
        assert_eq!(
            (
                bare.host.as_str(),
                bare.port,
                bare.user.as_str(),
                bare.database.as_str()
            ),
            ("localhost", 5432, "ana", "ana")
        );
        assert_eq!(bare.sslmode, SslMode::Prefer);

        let from_env = Resolved::of(
            &named(),
            env(&[
                ("PGHOST", "db"),
                ("PGPORT", "6543"),
                ("PGUSER", "svc"),
                ("USER", "ana"),
                ("PGDATABASE", "orders"),
                ("PGSSLMODE", "verify-full"),
            ]),
        );
        assert_eq!(
            (
                from_env.host.as_str(),
                from_env.port,
                from_env.user.as_str()
            ),
            ("db", 6543, "svc")
        );
        assert_eq!(from_env.database, "orders");
        assert_eq!(from_env.sslmode, SslMode::VerifyFull);
    }

    #[test]
    fn a_stated_field_outranks_the_environment() {
        let stated = ConnectionConfig {
            host: Some("stated".to_owned()),
            port: NonZeroU16::new(7000),
            user: Some("u".to_owned()),
            database: Some("d".to_owned()),
            sslmode: Some(SslMode::Disable),
            ..named()
        };
        let resolved = Resolved::of(&stated, env(&[("PGHOST", "env"), ("PGSSLMODE", "require")]));

        assert_eq!(
            (
                resolved.host.as_str(),
                resolved.port,
                resolved.user.as_str()
            ),
            ("stated", 7000, "u")
        );
        assert_eq!(resolved.database, "d");
        assert_eq!(resolved.sslmode, SslMode::Disable);
    }

    #[test]
    fn only_a_single_read_is_wrapped_for_paging() {
        assert_eq!(pageable(&read("select 1;  ")), Some("select 1"));
        assert_eq!(
            pageable(&read("WITH x AS (SELECT 1) TABLE x")),
            Some("WITH x AS (SELECT 1) TABLE x")
        );
        assert_eq!(pageable(&read("(SELECT 1)")), None);
        assert_eq!(pageable(&read("SELECT 1; SELECT 2")), None);
        assert_eq!(pageable(&read("SHOW search_path")), None);
        assert_eq!(
            pageable(&Statement {
                sql: "SELECT 1".to_owned(),
                kind: StatementKind::Write,
            }),
            None
        );
    }

    #[test]
    fn values_take_their_column_type_and_fall_back_to_text() {
        assert_eq!(value(None, Some(&Type::INT4)), Value::Null);
        assert_eq!(value(Some("t"), Some(&Type::BOOL)), Value::Boolean(true));
        assert_eq!(value(Some("-7"), Some(&Type::INT8)), Value::Int(-7));
        assert_eq!(
            value(Some("\\xdeadbeef"), Some(&Type::BYTEA)),
            Value::Blob(vec![0xde, 0xad, 0xbe, 0xef])
        );
        assert_eq!(
            value(Some("\\x"), Some(&Type::BYTEA)),
            Value::Blob(Vec::new())
        );
        assert_eq!(
            value(Some("3.14159265358979323846"), Some(&Type::NUMERIC)),
            Value::Numeric("3.14159265358979323846".to_owned())
        );
        assert!(matches!(value(Some("NaN"), Some(&Type::FLOAT8)), Value::Float(f) if f.is_nan()));
        assert_eq!(
            value(Some("Infinity"), Some(&Type::FLOAT8)),
            Value::Float(f64::INFINITY)
        );
        assert_eq!(value(Some("{1,2}"), None), Value::Text("{1,2}".to_owned()));
    }

    #[test]
    fn the_offset_round_trips_through_the_cursor() {
        assert_eq!(offset_of(&PagingState(500_u64.to_be_bytes().to_vec())), 500);
        assert_eq!(offset_of(&PagingState(vec![1])), 0);
    }
}
