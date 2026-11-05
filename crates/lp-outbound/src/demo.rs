//! The scripted database the app runs against until the Postgres adapter
//! lands (docs/plan.md, Phase 0). Mirrors the `shop` schema in `docker/seed`.

use std::collections::BTreeMap;

use lp_domain::model::{
    Column, CqlValue, Keyspace, Page, PrimaryKey, Replication, Row, Schema, Table, TableOptions,
};
use lp_domain::posture::Posture;

use crate::fake::FakeCluster;

#[must_use]
pub fn cluster() -> FakeCluster {
    FakeCluster::new(schema(), Posture::Writable).returning(customers())
}

fn column(name: &str, ty: &str) -> Column {
    Column {
        name: name.to_owned(),
        cql_type: ty.to_owned(),
    }
}

fn table(name: &str, key: &[(&str, &str)], columns: &[(&str, &str)]) -> (String, Table) {
    let table = Table {
        name: name.to_owned(),
        primary_key: PrimaryKey {
            partition: key.iter().map(|(n, t)| column(n, t)).collect(),
            clustering: Vec::new(),
        },
        static_columns: Vec::new(),
        regular_columns: columns.iter().map(|(n, t)| column(n, t)).collect(),
        indexes: Vec::new(),
        options: TableOptions::new(BTreeMap::new()),
    };
    (name.to_owned(), table)
}

fn schema() -> Schema {
    let shop = Keyspace {
        name: "shop".to_owned(),
        replication: Replication::Other {
            class: "postgres".to_owned(),
        },
        tables: BTreeMap::from([
            table(
                "customer",
                &[("id", "bigint")],
                &[
                    ("email", "text"),
                    ("name", "text"),
                    ("created_at", "timestamptz"),
                ],
            ),
            table(
                "product",
                &[("sku", "text")],
                &[("title", "text"), ("price", "numeric"), ("tags", "text[]")],
            ),
            table(
                "order_line",
                &[("order_id", "bigint"), ("line_no", "int")],
                &[
                    ("customer_id", "bigint"),
                    ("sku", "text"),
                    ("quantity", "int"),
                ],
            ),
        ]),
    };
    Schema {
        keyspaces: BTreeMap::from([("shop".to_owned(), shop)]),
    }
}

fn customers() -> Page {
    Page {
        columns: ["id", "email", "name"].map(str::to_owned).to_vec(),
        rows: (1..=50)
            .map(|id| Row {
                values: vec![
                    CqlValue::Int(id),
                    CqlValue::Text(format!("user{id}@example.com")),
                    CqlValue::Text(format!("User {id}")),
                ],
            })
            .collect(),
        next: None,
    }
}
