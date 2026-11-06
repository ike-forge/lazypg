//! The integration tier: the `docker/` fixture. `mise run db:up`, then
//! `mise run test:it`. `LP_PG_PORT` moves the port, as it does the fixture.
#![cfg(feature = "integration")]

use std::time::{Duration, Instant};

use lp_domain::config::{
    ConnectionConfig, ConnectionName, Credential, DriverConfig, Millis, PageSize, SslMode,
};
use lp_domain::connection::Transaction;
use lp_domain::model::{
    ColumnDefault, ConnId, ConstraintKind, DbError, Page, RelationKind, RelationRef, Statement,
    StatementKind, Value,
};
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
        sql: sql.to_owned(),
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

#[allow(
    clippy::unwrap_used,
    reason = "allow-unwrap-in-tests only covers #[test]-attributed items, not helpers they call"
)]
async fn catalog() -> lp_domain::model::Catalog {
    let db = connected(Posture::ReadOnly, DriverConfig::default()).await;
    db.describe(conn()).await.unwrap()
}

#[tokio::test]
async fn the_catalog_lists_user_schemas_and_relations_of_every_kind() {
    let catalog = catalog().await;

    let names: Vec<_> = catalog.schemas.keys().map(String::as_str).collect();
    assert_eq!(names, ["Edge Cases", "public", "shop", "zoo"]);

    let kind = |schema: &str, table: &str| catalog.schemas[schema].tables[table].kind;
    assert_eq!(kind("shop", "customer"), RelationKind::Table);
    assert_eq!(kind("shop", "customer_totals"), RelationKind::View);
    assert_eq!(
        kind("Edge Cases", "no_pk_summary"),
        RelationKind::MaterializedView
    );
    assert_eq!(kind("Edge Cases", "measurement"), RelationKind::Partitioned);

    let edge = &catalog.schemas["Edge Cases"];
    assert!(
        !edge.tables.contains_key("measurement_2026"),
        "a partition is its parent's"
    );
    assert_eq!(edge.tables["measurement"].partitions, ["measurement_2026"]);
    assert!(
        catalog.schemas["shop"].tables["customer_totals"]
            .definition
            .as_deref()
            .is_some_and(|sql| sql.contains("sum(")),
        "a view carries its query"
    );
}

#[tokio::test]
async fn the_catalog_places_itself_on_a_server_of_connectable_databases() {
    let server = catalog()
        .await
        .server
        .expect("a live catalog knows its server");

    assert_eq!(server.current, "lazypg");
    assert_eq!(
        server.databases,
        ["analytics", "lazypg", "postgres"],
        "templates take no sessions, so they are not listed"
    );
}

#[tokio::test]
async fn a_connection_naming_another_database_reads_that_one() {
    let db = Postgres::new();
    let target = ConnectionConfig {
        database: Some("analytics".to_owned()),
        ..target(Posture::ReadOnly)
    };
    db.connect(conn(), &target, &DriverConfig::default())
        .await
        .expect("analytics accepts the fixture user");

    let catalog = db.describe(conn()).await.expect("describes");

    assert_eq!(
        catalog.server.map(|server| server.current).as_deref(),
        Some("analytics")
    );
    assert!(catalog.schemas["metrics"].tables.contains_key("event"));
    assert!(!catalog.schemas.contains_key("shop"), "shop is lazypg's");
}

#[tokio::test]
async fn columns_carry_type_nullability_and_default_in_table_order() {
    let catalog = catalog().await;
    let customer = &catalog.schemas["shop"].tables["customer"];

    let names: Vec<_> = customer.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "email", "name", "created_at"]);
    let id = &customer.columns[0];
    assert_eq!(id.data_type, "bigint");
    assert!(!id.nullable);
    assert_eq!(id.default, Some(ColumnDefault::Identity { always: true }));
    assert_eq!(
        customer.columns[3].default,
        Some(ColumnDefault::Expression("now()".to_owned()))
    );
    let edge = &catalog.schemas["Edge Cases"].tables["measurement"];
    assert_eq!(edge.partition_key.as_deref(), Some("RANGE (logdate)"));
    assert_eq!(
        catalog.schemas["zoo"].tables["everything"].columns[7].data_type,
        "character varying(16)"
    );
}

#[tokio::test]
async fn keys_are_read_in_key_order_and_foreign_keys_from_both_ends() {
    let catalog = catalog().await;
    let shop = &catalog.schemas["shop"];
    let line = &shop.tables["order_line"];

    let key = line.primary_key.as_ref().expect("order_line has a key");
    assert_eq!(key.columns, ["order_id", "line_no"]);
    assert!(
        catalog.schemas["Edge Cases"].tables["no_pk"]
            .primary_key
            .is_none()
    );

    let targets: Vec<_> = line
        .foreign_keys
        .iter()
        .map(|key| {
            (
                key.columns.clone(),
                key.target.table.clone(),
                key.target_columns.clone(),
            )
        })
        .collect();
    assert_eq!(
        targets,
        [
            (
                vec!["customer_id".to_owned()],
                "customer".to_owned(),
                vec!["id".to_owned()]
            ),
            (
                vec!["sku".to_owned()],
                "product".to_owned(),
                vec!["sku".to_owned()]
            ),
        ]
    );
    assert_eq!(line.foreign_keys[0].on_delete, "NO ACTION");

    let customer = RelationRef {
        schema: "shop".to_owned(),
        table: "customer".to_owned(),
    };
    let incoming: Vec<_> = catalog
        .referencing(&customer)
        .map(|(from, _)| from.table)
        .collect();
    assert_eq!(incoming, ["order_line"]);
}

#[tokio::test]
async fn indexes_and_constraints_are_read_as_postgres_renders_them() {
    let catalog = catalog().await;
    let shop = &catalog.schemas["shop"];
    let line = &shop.tables["order_line"];

    let index = line
        .indexes
        .iter()
        .find(|index| index.name == "order_line_customer_idx")
        .expect("the seed creates it");
    assert_eq!(index.keys, ["customer_id"]);
    assert_eq!(index.method, "btree");
    assert!(!index.unique && !index.primary);
    assert!(
        index
            .definition
            .starts_with("CREATE INDEX order_line_customer_idx")
    );
    assert!(line.indexes.iter().any(|index| index.primary));

    let unique: Vec<_> = shop.tables["customer"]
        .constraints
        .iter()
        .map(|c| (c.kind, c.definition.as_str()))
        .collect();
    assert_eq!(unique, [(ConstraintKind::Unique, "UNIQUE (email)")]);
    assert!(
        shop.tables["product"]
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Check && c.definition.contains("price >="))
    );
}

#[tokio::test]
async fn identity_sequences_are_owned_by_their_column() {
    let catalog = catalog().await;

    let sequences: Vec<_> = catalog.schemas["shop"]
        .sequences
        .iter()
        .map(|s| (s.name.as_str(), s.data_type.as_str(), s.owned_by.as_deref()))
        .collect();
    assert_eq!(
        sequences,
        [("customer_id_seq", "bigint", Some("customer.id"))]
    );
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

    assert_eq!(cell(0, "b"), Value::Boolean(true));
    assert_eq!(cell(0, "i8"), Value::Int(i64::MAX));
    assert_eq!(
        cell(0, "n"),
        Value::Numeric("3.14159265358979323846264338327950288".to_owned())
    );
    assert!(matches!(cell(0, "f8"), Value::Float(f) if f.is_nan()));
    assert_eq!(cell(0, "by"), Value::Blob(vec![0xde, 0xad, 0xbe, 0xef]));
    assert!(matches!(cell(0, "u"), Value::Uuid(u) if u.len() == 36));
    assert_eq!(cell(0, "md"), Value::Array("{{1,2},{3,4}}".to_owned()));
    assert_eq!(cell(0, "arr"), Value::Array("{1,2,3}".to_owned()));
    assert_eq!(cell(0, "p"), Value::Text("(1.5,2.5)".to_owned()));
    assert_eq!(cell(0, "jb"), Value::Json(r#"{"b": [1, 2]}"#.to_owned()));

    assert_eq!(cell(1, "b"), Value::Null);
    assert_eq!(cell(1, "by"), Value::Blob(Vec::new()));
    assert_eq!(cell(1, "t"), Value::Text("multi\nline\ttab".to_owned()));
    assert_eq!(cell(1, "f8"), Value::Float(f64::INFINITY));
    assert_eq!(cell(1, "d"), Value::Text("infinity".to_owned()));
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
            assert_eq!(page.rows[0].values[0], Value::Int(7_001 / 3 + 1));
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
        Value::Int(n) => n,
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
        Value::Int(1)
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

#[tokio::test]
async fn every_value_read_back_spells_a_literal_postgres_finds_and_accepts() {
    let db = connected(Posture::Writable, DriverConfig::default()).await;
    let catalog = db.describe(conn()).await.expect("describes");
    let table = &catalog.schemas["zoo"].tables["everything"];
    let page = query(&db, "SELECT * FROM zoo.everything WHERE id = 1").await;

    for (at, column) in table.columns.iter().enumerate() {
        let value = &page.rows[0].values[at];
        let literal = lp_domain::literal::of(&column.data_type, value);
        let name = lp_domain::create::ident(&column.name);

        // json has no `=`; floats are compared by `literal::key`'s refusal.
        if column.data_type != "json" && lp_domain::literal::key(&column.data_type, value).is_some()
        {
            let found = query(
                &db,
                &format!("SELECT count(*) FROM zoo.everything WHERE {name} = {literal}"),
            )
            .await;
            assert_eq!(
                found.rows[0].values[0],
                Value::Int(1),
                "{} {literal} finds its own row",
                column.name
            );
        }

        let written = db
            .execute(
                conn(),
                &Statement {
                    sql: format!(
                        "BEGIN; UPDATE zoo.everything SET {name} = {literal} WHERE id = 1; ROLLBACK;"
                    ),
                    kind: StatementKind::Write,
                },
            )
            .await;
        assert!(written.is_ok(), "{} = {literal}: {written:?}", column.name);
    }
}

#[tokio::test]
async fn rows_yanked_as_inserts_recreate_themselves_in_a_copy() {
    let db = connected(Posture::Writable, DriverConfig::default()).await;
    let catalog = db.describe(conn()).await.expect("describes");
    let table = &catalog.schemas["zoo"].tables["everything"];
    let page = query(&db, "SELECT * FROM zoo.everything ORDER BY id").await;
    assert!(!page.rows.is_empty(), "the zoo is seeded");
    let copy = format!("yank_{}", std::process::id());
    let inserts =
        lp_domain::yank::inserts(&copy, table, &page.columns, &page.rows).expect("columns");
    let write = |sql: String| Statement {
        sql,
        kind: StatementKind::Write,
    };

    db.execute(
        conn(),
        &write(format!(
            "CREATE SCHEMA {copy}; \
             CREATE TABLE {copy}.everything (LIKE zoo.everything INCLUDING ALL); {inserts}"
        )),
    )
    .await
    .expect("the copy takes every insert");
    let matched = query(
        &db,
        &format!(
            "SELECT count(*) FROM zoo.everything AS z \
             JOIN {copy}.everything AS c ON z::text = c::text"
        ),
    )
    .await;
    db.execute(conn(), &write(format!("DROP SCHEMA {copy} CASCADE")))
        .await
        .expect("drops the copy");

    assert_eq!(
        matched.rows[0].values[0],
        Value::Int(i64::try_from(page.rows.len()).expect("fits"))
    );
}

#[tokio::test]
async fn the_probe_follows_a_transaction_through_open_failed_and_rolled_back() {
    let db = connected(Posture::Writable, DriverConfig::default()).await;
    let run = |sql: &str| Statement {
        sql: sql.to_owned(),
        kind: StatementKind::Write,
    };
    let probed = async || db.transaction(conn()).await.expect("answers");

    assert_eq!(probed().await, Transaction::Idle);

    db.execute(conn(), &run("BEGIN")).await.expect("begins");
    assert_eq!(probed().await, Transaction::Open);

    let failed = db.execute(conn(), &run("SELECT 1 / 0")).await;
    assert!(failed.is_err());
    assert_eq!(probed().await, Transaction::Failed);

    db.execute(conn(), &run("ROLLBACK"))
        .await
        .expect("rolls back");
    assert_eq!(probed().await, Transaction::Idle);
}
