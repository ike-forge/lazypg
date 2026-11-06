//! A relation's definition as the SQL that would create it, rebuilt from the
//! catalog: Postgres has no `pg_get_tabledef`. Indexes and constraints use the
//! server's own rendering, so only the table's shell is ours.

use std::fmt::Write as _;

use crate::model::{Column, ForeignKey, RelationKind, RelationRef, Schema, Table};

/// Every keyword Postgres reserves in some position: quoting one that only
/// needed it elsewhere costs nothing, missing one breaks the statement.
const RESERVED: &[&str] = &[
    "all",
    "analyse",
    "analyze",
    "and",
    "any",
    "array",
    "as",
    "asc",
    "asymmetric",
    "authorization",
    "binary",
    "both",
    "case",
    "cast",
    "check",
    "collate",
    "collation",
    "column",
    "concurrently",
    "constraint",
    "create",
    "cross",
    "current_catalog",
    "current_date",
    "current_role",
    "current_schema",
    "current_time",
    "current_timestamp",
    "current_user",
    "default",
    "deferrable",
    "desc",
    "distinct",
    "do",
    "else",
    "end",
    "except",
    "false",
    "fetch",
    "for",
    "foreign",
    "freeze",
    "from",
    "full",
    "grant",
    "group",
    "having",
    "ilike",
    "in",
    "initially",
    "inner",
    "intersect",
    "into",
    "is",
    "isnull",
    "join",
    "lateral",
    "leading",
    "left",
    "like",
    "limit",
    "localtime",
    "localtimestamp",
    "natural",
    "not",
    "notnull",
    "null",
    "offset",
    "on",
    "only",
    "or",
    "order",
    "outer",
    "overlaps",
    "placing",
    "primary",
    "references",
    "returning",
    "right",
    "select",
    "session_user",
    "similar",
    "some",
    "symmetric",
    "system_user",
    "table",
    "tablesample",
    "then",
    "to",
    "trailing",
    "true",
    "union",
    "unique",
    "user",
    "using",
    "variadic",
    "verbose",
    "when",
    "where",
    "window",
    "with",
];

/// Bare when Postgres would read it back unchanged: lower case, a letter or
/// underscore first, not reserved.
#[must_use]
pub fn ident(name: &str) -> String {
    let mut chars = name.chars();
    let plain = chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$')
        && !RESERVED.contains(&name);

    if plain {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

fn qualified(schema: &str, table: &str) -> String {
    format!("{}.{}", ident(schema), ident(table))
}

fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn names(columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| ident(column))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Lines, so the Structure panel can draw and yank them one at a time.
#[must_use]
pub fn statements(schema: &Schema, table: &Table) -> Vec<String> {
    let name = qualified(&schema.name, &table.name);
    let mut lines = match table.kind {
        RelationKind::View => view("CREATE VIEW", &name, table),
        RelationKind::MaterializedView => view("CREATE MATERIALIZED VIEW", &name, table),
        RelationKind::Table | RelationKind::Partitioned | RelationKind::Foreign => {
            shell(&name, table)
        }
    };

    let constraint_names: Vec<&str> = table
        .constraints
        .iter()
        .map(|constraint| constraint.name.as_str())
        .collect();
    // An index behind a key or a unique constraint is created with it.
    let indexes: Vec<String> = table
        .indexes
        .iter()
        .filter(|index| !index.primary && !constraint_names.contains(&index.name.as_str()))
        .map(|index| format!("{};", index.definition))
        .collect();
    if !indexes.is_empty() {
        lines.push(String::new());
        lines.extend(indexes);
    }

    let comments = comments(&name, table);
    if !comments.is_empty() {
        lines.push(String::new());
        lines.extend(comments);
    }

    if !table.partitions.is_empty() {
        lines.push(String::new());
        lines.push("-- Partitions; their bounds are on each partition:".to_owned());
        lines.extend(
            table
                .partitions
                .iter()
                .map(|partition| format!("--   {}", qualified(&schema.name, partition))),
        );
    }

    lines
}

/// `pg_get_viewdef` ends its query with the `;`.
fn view(verb: &str, name: &str, table: &Table) -> Vec<String> {
    std::iter::once(format!("{verb} {name} AS"))
        .chain(
            table
                .definition
                .iter()
                .flat_map(|definition| definition.lines())
                .map(|line| format!("  {}", line.trim_start())),
        )
        .collect()
}

fn shell(name: &str, table: &Table) -> Vec<String> {
    let verb = if table.kind == RelationKind::Foreign {
        "CREATE FOREIGN TABLE"
    } else {
        "CREATE TABLE"
    };

    let mut items: Vec<String> = table.columns.iter().map(column).collect();
    if let Some(key) = &table.primary_key {
        items.push(format!(
            "CONSTRAINT {} PRIMARY KEY ({})",
            ident(&key.name),
            names(&key.columns)
        ));
    }
    items.extend(table.foreign_keys.iter().map(foreign_key));
    items.extend(table.constraints.iter().map(|constraint| {
        format!(
            "CONSTRAINT {} {}",
            ident(&constraint.name),
            constraint.definition
        )
    }));

    let last = items.len().saturating_sub(1);
    let mut lines = vec![format!("{verb} {name} (")];
    lines.extend(items.into_iter().enumerate().map(|(at, item)| {
        if at == last {
            format!("    {item}")
        } else {
            format!("    {item},")
        }
    }));

    let mut close = ")".to_owned();
    if let Some(key) = &table.partition_key {
        let _ = write!(close, " PARTITION BY {key}");
    }
    if table.kind == RelationKind::Foreign {
        lines.push(close);
        lines.push("-- SERVER and OPTIONS are not read from the catalog.".to_owned());
        return lines;
    }
    close.push(';');
    lines.push(close);
    lines
}

fn column(column: &Column) -> String {
    let mut line = format!("{} {}", ident(&column.name), column.data_type);
    if let Some(default) = &column.default {
        line.push(' ');
        line.push_str(&default.clause());
    }
    if !column.nullable {
        line.push_str(" NOT NULL");
    }
    line
}

fn foreign_key(key: &ForeignKey) -> String {
    let RelationRef { schema, table } = &key.target;
    let mut clause = format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        ident(&key.name),
        names(&key.columns),
        qualified(schema, table),
        names(&key.target_columns)
    );
    for (event, action) in [("UPDATE", &key.on_update), ("DELETE", &key.on_delete)] {
        if action != "NO ACTION" {
            let _ = write!(clause, " ON {event} {action}");
        }
    }
    clause
}

fn comments(name: &str, table: &Table) -> Vec<String> {
    let object = match table.kind {
        RelationKind::View => "VIEW",
        RelationKind::MaterializedView => "MATERIALIZED VIEW",
        RelationKind::Foreign => "FOREIGN TABLE",
        RelationKind::Table | RelationKind::Partitioned => "TABLE",
    };

    table
        .comment
        .iter()
        .map(|comment| format!("COMMENT ON {object} {name} IS {};", literal(comment)))
        .chain(table.columns.iter().filter_map(|column| {
            column.comment.as_ref().map(|comment| {
                format!(
                    "COMMENT ON COLUMN {name}.{} IS {};",
                    ident(&column.name),
                    literal(comment)
                )
            })
        }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ColumnDefault, Constraint, ConstraintKind, Index, PrimaryKey};

    fn not_null(name: &str, data_type: &str) -> Column {
        Column {
            nullable: false,
            ..Column::new(name, data_type)
        }
    }

    fn order_line() -> Table {
        Table {
            name: "order_line".to_owned(),
            columns: vec![
                Column {
                    default: Some(ColumnDefault::Identity { always: true }),
                    ..not_null("order_id", "bigint")
                },
                not_null("line_no", "integer"),
                not_null("customer_id", "bigint"),
                Column {
                    default: Some(ColumnDefault::Expression("'none'::text".to_owned())),
                    comment: Some("What the customer wrote.".to_owned()),
                    ..Column::new("select", "text")
                },
            ],
            primary_key: Some(PrimaryKey {
                name: "order_line_pkey".to_owned(),
                columns: vec!["order_id".to_owned(), "line_no".to_owned()],
            }),
            foreign_keys: vec![ForeignKey {
                name: "order_line_customer_id_fkey".to_owned(),
                columns: vec!["customer_id".to_owned()],
                target: RelationRef {
                    schema: "shop".to_owned(),
                    table: "customer".to_owned(),
                },
                target_columns: vec!["id".to_owned()],
                on_update: "NO ACTION".to_owned(),
                on_delete: "CASCADE".to_owned(),
            }],
            indexes: vec![
                Index {
                    name: "order_line_pkey".to_owned(),
                    definition: "CREATE UNIQUE INDEX order_line_pkey ON shop.order_line USING btree (order_id, line_no)".to_owned(),
                    keys: Vec::new(),
                    unique: true,
                    primary: true,
                    method: "btree".to_owned(),
                    predicate: None,
                },
                Index {
                    name: "order_line_one_note".to_owned(),
                    definition: "CREATE UNIQUE INDEX order_line_one_note ON shop.order_line USING btree (\"select\")".to_owned(),
                    keys: Vec::new(),
                    unique: true,
                    primary: false,
                    method: "btree".to_owned(),
                    predicate: None,
                },
                Index {
                    name: "order_line_customer_idx".to_owned(),
                    definition: "CREATE INDEX order_line_customer_idx ON shop.order_line USING btree (customer_id)".to_owned(),
                    keys: Vec::new(),
                    unique: false,
                    primary: false,
                    method: "btree".to_owned(),
                    predicate: None,
                },
            ],
            constraints: vec![
                Constraint {
                    name: "order_line_line_no_check".to_owned(),
                    kind: ConstraintKind::Check,
                    definition: "CHECK ((line_no >= 0))".to_owned(),
                },
                Constraint {
                    name: "order_line_one_note".to_owned(),
                    kind: ConstraintKind::Unique,
                    definition: "UNIQUE (\"select\")".to_owned(),
                },
            ],
            comment: Some("One line per product, it's said.".to_owned()),
            ..Table::default()
        }
    }

    #[test]
    fn a_table_is_rebuilt_with_its_keys_constraints_indexes_and_comments() {
        assert_eq!(
            statements(&Schema::empty("shop"), &order_line()),
            [
                "CREATE TABLE shop.order_line (",
                "    order_id bigint GENERATED ALWAYS AS IDENTITY NOT NULL,",
                "    line_no integer NOT NULL,",
                "    customer_id bigint NOT NULL,",
                "    \"select\" text DEFAULT 'none'::text,",
                "    CONSTRAINT order_line_pkey PRIMARY KEY (order_id, line_no),",
                "    CONSTRAINT order_line_customer_id_fkey FOREIGN KEY (customer_id) REFERENCES shop.customer (id) ON DELETE CASCADE,",
                "    CONSTRAINT order_line_line_no_check CHECK ((line_no >= 0)),",
                "    CONSTRAINT order_line_one_note UNIQUE (\"select\")",
                ");",
                "",
                "CREATE INDEX order_line_customer_idx ON shop.order_line USING btree (customer_id);",
                "",
                "COMMENT ON TABLE shop.order_line IS 'One line per product, it''s said.';",
                "COMMENT ON COLUMN shop.order_line.\"select\" IS 'What the customer wrote.';",
            ],
            "the key's and the unique constraint's indexes come with them, so only the other is created"
        );
    }

    #[test]
    fn a_partitioned_table_states_its_key_and_lists_its_partitions() {
        let table = Table {
            name: "measurement".to_owned(),
            kind: RelationKind::Partitioned,
            columns: vec![not_null("logdate", "date")],
            partition_key: Some("RANGE (logdate)".to_owned()),
            partitions: vec!["measurement_2026".to_owned()],
            ..Table::default()
        };

        assert_eq!(
            statements(&Schema::empty("Edge Cases"), &table),
            [
                "CREATE TABLE \"Edge Cases\".measurement (",
                "    logdate date NOT NULL",
                ") PARTITION BY RANGE (logdate);",
                "",
                "-- Partitions; their bounds are on each partition:",
                "--   \"Edge Cases\".measurement_2026",
            ]
        );
    }

    #[test]
    fn a_view_is_its_query() {
        let view = Table {
            name: "totals".to_owned(),
            kind: RelationKind::View,
            definition: Some(" SELECT c.id\n   FROM shop.customer c;".to_owned()),
            ..Table::default()
        };

        assert_eq!(
            statements(&Schema::empty("shop"), &view),
            [
                "CREATE VIEW shop.totals AS",
                "  SELECT c.id",
                "  FROM shop.customer c;",
            ]
        );
    }

    #[test]
    fn an_identifier_is_quoted_exactly_when_postgres_would_fold_or_refuse_it() {
        assert_eq!(ident("order_line"), "order_line");
        assert_eq!(ident("_x1$"), "_x1$");
        assert_eq!(ident("Mixed Case"), "\"Mixed Case\"");
        assert_eq!(ident("user"), "\"user\"");
        assert_eq!(ident("1st"), "\"1st\"");
        assert_eq!(ident("ünïcødé"), "\"ünïcødé\"");
        assert_eq!(ident("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
