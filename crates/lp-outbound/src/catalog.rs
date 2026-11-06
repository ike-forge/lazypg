//! Introspection: [`lp_domain::introspection`]'s queries, run together over
//! the session's pipeline, their rows into our [`Catalog`].

use lp_domain::introspection::{
    COLUMNS, CONSTRAINTS, FUNCTIONS, INDEXES, PARTITIONS, RELATIONS, SCHEMAS, SEQUENCES, SERVER,
};
use lp_domain::model::{
    Catalog, Column, ColumnDefault, Constraint, ConstraintKind, DbError, ForeignKey, Function,
    FunctionKind, Index, PrimaryKey, RelationKind, RelationRef, Schema, Sequence, Server, Table,
};
use tokio_postgres::{Client, Row};

pub(crate) async fn describe(client: &Client) -> Result<Catalog, DbError> {
    let query = |sql: &'static str| async move {
        client
            .query(sql, &[])
            .await
            .map_err(|error| crate::postgres::db_error(&error))
    };
    let (
        server,
        schemas,
        relations,
        columns,
        constraints,
        indexes,
        partitions,
        sequences,
        functions,
    ) = tokio::try_join!(
        query(SERVER),
        query(SCHEMAS),
        query(RELATIONS),
        query(COLUMNS),
        query(CONSTRAINTS),
        query(INDEXES),
        query(PARTITIONS),
        query(SEQUENCES),
        query(FUNCTIONS),
    )?;

    let mut catalog = Catalog {
        server: server.first().map(server_of),
        schemas: schemas
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                (name.clone(), Schema::empty(&name))
            })
            .collect(),
    };

    for row in &relations {
        let Some(schema) = catalog.schemas.get_mut(&row.get::<_, String>(0)) else {
            continue;
        };
        let name: String = row.get(1);
        let table = Table {
            name: name.clone(),
            kind: relation_kind(&row.get::<_, String>(2)),
            comment: row.get(3),
            definition: row.get(4),
            partition_key: row.get(5),
            ..Table::default()
        };
        schema.tables.insert(name, table);
    }

    let mut tables = Tables(&mut catalog);

    for row in &columns {
        if let Some(table) = tables.at(row) {
            table.columns.push(Column {
                name: row.get(2),
                data_type: row.get(3),
                nullable: row.get(4),
                default: column_default(
                    &row.get::<_, String>(5),
                    &row.get::<_, String>(6),
                    row.get(7),
                ),
                comment: row.get(8),
            });
        }
    }

    for row in &constraints {
        if let Some(table) = tables.at(row) {
            constraint(table, row);
        }
    }

    for row in &indexes {
        if let Some(table) = tables.at(row) {
            table.indexes.push(Index {
                name: row.get(2),
                definition: row.get(3),
                keys: row.get(4),
                unique: row.get(5),
                primary: row.get(6),
                method: row.get(7),
                predicate: row.get(8),
            });
        }
    }

    for row in &partitions {
        if let Some(table) = tables.at(row) {
            table.partitions.push(row.get(2));
        }
    }

    schema_objects(&mut catalog, &sequences, &functions);

    Ok(catalog)
}

fn server_of(row: &Row) -> Server {
    let current: String = row.get(0);
    let mut databases: Vec<String> = row.get(1);
    // Connected, so allowed, whatever the privilege check said.
    if !databases.contains(&current) {
        databases.push(current.clone());
        databases.sort();
    }
    Server { current, databases }
}

fn schema_objects(catalog: &mut Catalog, sequences: &[Row], functions: &[Row]) {
    for row in sequences {
        if let Some(schema) = catalog.schemas.get_mut(&row.get::<_, String>(0)) {
            schema.sequences.push(Sequence {
                name: row.get(1),
                data_type: row.get(2),
                owned_by: row.get(3),
            });
        }
    }

    for row in functions {
        if let Some(schema) = catalog.schemas.get_mut(&row.get::<_, String>(0)) {
            schema.functions.push(Function {
                name: row.get(1),
                kind: function_kind(&row.get::<_, String>(2)),
                arguments: row.get(3),
                result: row.get::<_, Option<String>>(4).unwrap_or_default(),
            });
        }
    }
}

/// Rows lead with schema and relation name; a row for a relation not listed
/// (a race with DDL between queries) is skipped.
struct Tables<'a>(&'a mut Catalog);

impl Tables<'_> {
    fn at(&mut self, row: &Row) -> Option<&mut Table> {
        self.0
            .schemas
            .get_mut(&row.get::<_, String>(0))?
            .tables
            .get_mut(&row.get::<_, String>(1))
    }
}

fn constraint(table: &mut Table, row: &Row) {
    let name: String = row.get(2);
    let definition: String = row.get(4);
    let columns: Vec<String> = row.get(5);

    let kind = match row.get::<_, String>(3).as_str() {
        "p" => {
            table.primary_key = Some(PrimaryKey { name, columns });
            return;
        }
        "f" => {
            let target = row
                .get::<_, Option<String>>(6)
                .zip(row.get::<_, Option<String>>(7))
                .map(|(schema, table)| RelationRef { schema, table });
            if let Some(target) = target {
                table.foreign_keys.push(ForeignKey {
                    name,
                    columns,
                    target,
                    target_columns: row.get(8),
                    on_update: action(&row.get::<_, String>(9)).to_owned(),
                    on_delete: action(&row.get::<_, String>(10)).to_owned(),
                });
            }
            return;
        }
        "u" => ConstraintKind::Unique,
        "x" => ConstraintKind::Exclusion,
        _ => ConstraintKind::Check,
    };

    table.constraints.push(Constraint {
        name,
        kind,
        definition,
    });
}

/// `attidentity` and `attgenerated` are empty strings when unset.
fn column_default(
    identity: &str,
    generated: &str,
    expression: Option<String>,
) -> Option<ColumnDefault> {
    match (identity, generated, expression) {
        ("a", _, _) => Some(ColumnDefault::Identity { always: true }),
        ("d", _, _) => Some(ColumnDefault::Identity { always: false }),
        (_, "s" | "v", Some(expression)) => Some(ColumnDefault::Generated(expression)),
        (_, _, expression) => expression.map(ColumnDefault::Expression),
    }
}

fn relation_kind(code: &str) -> RelationKind {
    match code {
        "p" => RelationKind::Partitioned,
        "v" => RelationKind::View,
        "m" => RelationKind::MaterializedView,
        "f" => RelationKind::Foreign,
        _ => RelationKind::Table,
    }
}

fn function_kind(code: &str) -> FunctionKind {
    match code {
        "p" => FunctionKind::Procedure,
        "a" => FunctionKind::Aggregate,
        "w" => FunctionKind::Window,
        _ => FunctionKind::Function,
    }
}

/// `pg_constraint.confupdtype` and `confdeltype`.
fn action(code: &str) -> &'static str {
    match code {
        "r" => "RESTRICT",
        "c" => "CASCADE",
        "n" => "SET NULL",
        "d" => "SET DEFAULT",
        _ => "NO ACTION",
    }
}
