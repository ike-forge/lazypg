//! The integration tier: the `docker/` fixture. `mise run db:up`, then
//! `mise run test:it`. `LP_PG_PORT` moves the port, as it does the fixture.
#![cfg(feature = "integration")]

use std::time::{Duration, Instant};

use lp_domain::config::{
    ConnectionConfig, ConnectionName, Credential, DriverConfig, Millis, PageSize, SslMode,
};
use lp_domain::model::{ConnId, CqlValue, DbError, Page, Statement, StatementKind};
use lp_domain::ports::Cluster;
use lp_domain::posture::Posture;
use lp_outbound::postgres::Postgres;

/// One id serves every test: each `Postgres` keeps its own connections.
fn conn() -> ConnId {
    static CONN: std::sync::OnceLock<ConnId> = std::sync::OnceLock::new();
    *CONN.get_or_init(ConnId::next)
}

#[allow(
    clippy::unwrap_used,
    reason = "allow-unwrap-in-tests only covers #[test]-attributed items, not helpers they call"
)]
fn target(posture: Posture) -> ConnectionConfig {
    let port = std::env::var("LP_PG_PORT").unwrap_or_else(|_| "5432".to_owned());
    ConnectionConfig {
        host: Some("127.0.0.1".to_owned()),
        port: Some(port.parse().unwrap()),
        user: Some("lazypg".to_owned()),
        password: Some(Credential::Literal("lazypg".to_owned())),
        sslmode: Some(SslMode::Disable),
        posture,
        ..ConnectionConfig::named(ConnectionName::try_new("fixture").unwrap())
    }
}

#[allow(
    clippy::unwrap_used,
    reason = "allow-unwrap-in-tests only covers #[test]-attributed items, not helpers they call"
)]
async fn connected(posture: Posture, driver: DriverConfig) -> Postgres {
    let db = Postgres::new();
    db.connect(conn(), &target(posture), &driver).await.unwrap();
    db
}

fn read(sql: &str) -> Statement {
    Statement {
        cql: sql.to_owned(),
        kind: StatementKind::Read,
    }
}

#[allow(
    clippy::unwrap_used,
    reason = "allow-unwrap-in-tests only covers #[test]-attributed items, not helpers they call"
)]
async fn query(db: &Postgres, sql: &str) -> Page {
    db.execute(conn(), &read(sql)).await.unwrap()
}

#[tokio::test]
async fn the_schema_reads_keys_in_key_order_and_skips_partitions() {
    let db = connected(Posture::ReadOnly, DriverConfig::default()).await;
    let schema = db.describe(conn()).await.unwrap();

    let shop = &schema.schemas["shop"];
    let line = &shop.tables["order_line"];
    let key: Vec<_> = line
        .primary_key
        .partition
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(key, ["order_id", "line_no"]);
    assert_eq!(line.regular_columns[0].name, "customer_id");
    assert!(
        shop.tables.contains_key("customer_totals"),
        "views are listed"
    );

    let edge = &schema.schemas["Edge Cases"];
    assert!(edge.tables["no_pk"].primary_key.partition.is_empty());
    assert!(edge.tables.contains_key("measurement"));
    assert!(!edge.tables.contains_key("measurement_2026"));
    assert_eq!(
        edge.tables["Mixed Case"].primary_key.partition[0].data_type,
        "integer"
    );
    assert!(schema.schemas.contains_key("public"));
    assert!(!schema.schemas.contains_key("pg_catalog"));
}

#[tokio::test]
async fn every_type_in_the_zoo_reads_back() {
    let db = connected(Posture::ReadOnly, DriverConfig::default()).await;
    let page = query(&db, "SELECT * FROM zoo.everything ORDER BY id").await;

    assert_eq!(page.columns.len(), 23);
    let cell = |row: usize, name: &str| {
        let i = page.columns.iter().position(|c| c == name).unwrap();
        page.rows[row].values[i].clone()
    };

    assert_eq!(cell(0, "b"), CqlValue::Boolean(true));
    assert_eq!(cell(0, "i8"), CqlValue::Int(i64::MAX));
    assert_eq!(
        cell(0, "n"),
        CqlValue::Text("3.14159265358979323846264338327950288".to_owned())
    );
    assert!(matches!(cell(0, "f8"), CqlValue::Float(f) if f.is_nan()));
    assert_eq!(cell(0, "by"), CqlValue::Blob(vec![0xde, 0xad, 0xbe, 0xef]));
    assert!(matches!(cell(0, "u"), CqlValue::Uuid(u) if u.len() == 36));
    assert_eq!(cell(0, "md"), CqlValue::Text("{{1,2},{3,4}}".to_owned()));
    assert_eq!(cell(0, "p"), CqlValue::Text("(1.5,2.5)".to_owned()));
    assert_eq!(cell(0, "jb"), CqlValue::Text(r#"{"b": [1, 2]}"#.to_owned()));

    assert_eq!(cell(1, "b"), CqlValue::Null);
    assert_eq!(cell(1, "by"), CqlValue::Blob(Vec::new()));
    assert_eq!(cell(1, "t"), CqlValue::Text("multi\nline\ttab".to_owned()));
    assert_eq!(cell(1, "f8"), CqlValue::Float(f64::INFINITY));
    assert_eq!(cell(1, "d"), CqlValue::Text("infinity".to_owned()));
}

#[tokio::test]
async fn reads_page_by_offset_until_the_last_row() {
    let db = connected(Posture::ReadOnly, DriverConfig::default()).await;
    let stmt = read("SELECT order_id, line_no FROM shop.order_line ORDER BY 1, 2;");
    let size = PageSize::try_new(7_000).unwrap();

    let mut cursor = None;
    let mut rows = 0;
    let mut pages = 0;
    loop {
        let page = db
            .execute_paged(conn(), &stmt, size, cursor.as_ref())
            .await
            .unwrap();
        rows += page.rows.len();
        pages += 1;
        if pages == 2 {
            assert_eq!(page.rows[0].values[0], CqlValue::Int(7_001 / 3 + 1));
        }
        match page.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    assert_eq!((rows, pages), (19_998, 3));
}

#[tokio::test]
async fn a_statement_that_cannot_be_wrapped_runs_unpaged() {
    let db = connected(Posture::ReadOnly, DriverConfig::default()).await;
    let page = db
        .execute_paged(
            conn(),
            &read("SHOW server_version_num"),
            PageSize::try_new(1).unwrap(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(page.rows.len(), 1);
    assert!(page.next.is_none());
}

#[tokio::test]
async fn a_read_only_connection_is_refused_writes_by_the_server() {
    let db = connected(Posture::ReadOnly, DriverConfig::default()).await;
    // Classified as a read so the domain's rail is not what stops it.
    let result = db
        .execute(
            conn(),
            &read(
                "WITH gone AS (DELETE FROM shop.order_line RETURNING 1) SELECT count(*) FROM gone",
            ),
        )
        .await;

    assert_eq!(result, Err(DbError::ReadOnly));
}

#[tokio::test]
async fn a_wrong_password_is_an_authentication_failure() {
    let mut target = target(Posture::ReadOnly);
    target.password = Some(Credential::Literal("wrong".to_owned()));

    let result = Postgres::new()
        .connect(conn(), &target, &DriverConfig::default())
        .await;

    assert!(
        matches!(result, Err(DbError::Authentication(_))),
        "{result:?}"
    );
}

async fn sleeping(db: &Postgres) -> i64 {
    match query(
        db,
        "SELECT count(*) FROM pg_stat_activity WHERE query LIKE 'SELECT pg_sleep(30)%' AND state = 'active'",
    )
    .await
    .rows[0]
        .values[0]
    {
        CqlValue::Int(n) => n,
        ref other => panic!("{other:?}"),
    }
}

/// Polls rather than sleeps a fixed time: the integration tier waits on the
/// condition (conventions.md §4).
async fn until_sleeping_is(db: &Postgres, wanted: i64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while sleeping(db).await != wanted {
        assert!(
            Instant::now() < deadline,
            "pg_sleep count never reached {wanted}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn aborting_a_query_cancels_it_on_the_server() {
    let db = std::sync::Arc::new(connected(Posture::ReadOnly, DriverConfig::default()).await);
    let observer = connected(Posture::ReadOnly, DriverConfig::default()).await;

    let running = {
        let db = std::sync::Arc::clone(&db);
        tokio::spawn(async move { db.execute(conn(), &read("SELECT pg_sleep(30)")).await })
    };

    until_sleeping_is(&observer, 1).await;
    running.abort();
    until_sleeping_is(&observer, 0).await;

    // The connection survives its cancelled query.
    assert_eq!(
        query(&db, "SELECT 1").await.rows[0].values[0],
        CqlValue::Int(1)
    );
}

#[tokio::test]
async fn a_slow_query_times_out_and_is_cancelled() {
    let driver = DriverConfig {
        request_timeout: Millis::try_new(200).unwrap(),
        ..DriverConfig::default()
    };
    let db = connected(Posture::ReadOnly, driver).await;
    let observer = connected(Posture::ReadOnly, DriverConfig::default()).await;

    let result = db.execute(conn(), &read("SELECT pg_sleep(30)")).await;

    assert_eq!(result, Err(DbError::Timeout));
    until_sleeping_is(&observer, 0).await;
}
