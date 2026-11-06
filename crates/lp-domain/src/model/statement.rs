//! CQL statements as values: every action is shown before it runs (vision.md
//! §4, principle 3).

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub sql: String,
    pub kind: StatementKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    Read,
    Write,
    /// Gated by posture and by typed confirmation (vision.md §4, principle 6).
    Destructive,
    Catalog,
}

impl StatementKind {
    #[must_use]
    pub const fn mutates(self) -> bool {
        matches!(self, Self::Write | Self::Destructive | Self::Catalog)
    }

    #[must_use]
    pub const fn is_destructive(self) -> bool {
        matches!(self, Self::Destructive)
    }
}

pub type UpdatedRow = (Vec<(String, String)>, Vec<(String, String)>);

impl Statement {
    #[must_use]
    pub const fn mutates(&self) -> bool {
        self.kind.mutates()
    }

    /// Unbounded: the grid pages it and export streams it. Ordered by the
    /// key when there is one, so `LIMIT/OFFSET` pages neither repeat nor skip
    /// a row (architecture.md §6).
    #[must_use]
    pub fn select_all(schema: &str, table: &str, key: &[String]) -> Self {
        let ident = crate::create::ident;
        let mut sql = format!("SELECT * FROM {}.{}", ident(schema), ident(table));
        if !key.is_empty() {
            let columns: Vec<String> = key.iter().map(|column| ident(column)).collect();
            sql = format!("{sql} ORDER BY {}", columns.join(", "));
        }
        Self {
            sql,
            kind: StatementKind::Read,
        }
    }

    /// Quoted: a name read back from the cluster is only safe to interpolate
    /// quoted.
    #[must_use]
    pub fn drop_table(keyspace: &str, table: &str) -> Self {
        Self {
            sql: format!("DROP TABLE {}.{}", quoted(keyspace), quoted(table)),
            kind: StatementKind::Destructive,
        }
    }

    #[must_use]
    pub fn drop_keyspace(keyspace: &str) -> Self {
        Self {
            sql: format!("DROP KEYSPACE {}", quoted(keyspace)),
            kind: StatementKind::Destructive,
        }
    }

    /// `key` arrives spelled by [`crate::literal`] in key order. `Write`, not
    /// `Destructive`, because `kind_of` classifies a typed `DELETE` so; its
    /// confirmation belongs to the key that opened it.
    #[must_use]
    pub fn delete_row(keyspace: &str, table: &str, key: &[(String, String)]) -> Self {
        Self {
            sql: format!(
                "DELETE FROM {}.{} WHERE {}",
                quoted(keyspace),
                quoted(table),
                restriction(key)
            ),
            kind: StatementKind::Write,
        }
    }

    /// Logged: an unlogged batch that half-applies breaks the "3 rows" the
    /// confirmation promised. A single key is a bare `DELETE`.
    #[must_use]
    pub fn delete_rows(keyspace: &str, table: &str, keys: &[Vec<(String, String)>]) -> Self {
        if let [only] = keys {
            return Self::delete_row(keyspace, table, only);
        }

        // One transaction: a failure part-way commits none of them.
        let mut sql = "BEGIN;\n".to_owned();
        for key in keys {
            let _ = writeln!(
                sql,
                "DELETE FROM {}.{} WHERE {};",
                quoted(keyspace),
                quoted(table),
                restriction(key)
            );
        }
        sql.push_str("COMMIT;");

        Self {
            sql,
            kind: StatementKind::Write,
        }
    }

    /// `Write`, as `kind_of` classifies a typed `UPDATE`.
    #[must_use]
    pub fn update_row(
        keyspace: &str,
        table: &str,
        sets: &[(String, String)],
        key: &[(String, String)],
    ) -> Self {
        Self {
            sql: format!(
                "UPDATE {}.{} SET {} WHERE {}",
                quoted(keyspace),
                quoted(table),
                assignments(sets),
                restriction(key)
            ),
            kind: StatementKind::Write,
        }
    }

    /// An omitted column is left unset; writing `null` would store a tombstone.
    #[must_use]
    pub fn insert_row(keyspace: &str, table: &str, values: &[(String, String)]) -> Self {
        let columns: Vec<String> = values.iter().map(|(column, _)| quoted(column)).collect();
        let literals: Vec<&str> = values.iter().map(|(_, literal)| literal.as_str()).collect();

        Self {
            sql: format!(
                "INSERT INTO {}.{} ({}) VALUES ({})",
                quoted(keyspace),
                quoted(table),
                columns.join(", "),
                literals.join(", ")
            ),
            kind: StatementKind::Write,
        }
    }

    /// Logged, for [`Self::delete_rows`]' reason. A single row is a bare
    /// `UPDATE`.
    #[must_use]
    pub fn update_rows(keyspace: &str, table: &str, rows: &[UpdatedRow]) -> Self {
        if let [(sets, key)] = rows {
            return Self::update_row(keyspace, table, sets, key);
        }

        // One transaction: a failure part-way commits none of them.
        let mut sql = "BEGIN;\n".to_owned();
        for (sets, key) in rows {
            let _ = writeln!(
                sql,
                "UPDATE {}.{} SET {} WHERE {};",
                quoted(keyspace),
                quoted(table),
                assignments(sets),
                restriction(key)
            );
        }
        sql.push_str("COMMIT;");

        Self {
            sql,
            kind: StatementKind::Write,
        }
    }

    /// Every statement in the buffer is classified and the most restricted
    /// wins, so a `SELECT` ahead of a `DROP` cannot launder it.
    #[must_use]
    pub fn typed(sql: &str) -> Self {
        Self {
            sql: sql.to_owned(),
            kind: kind_of(sql),
        }
    }
}

/// `system_schema` reports true-case names, which CQL would fold unquoted.
fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn restriction(key: &[(String, String)]) -> String {
    key.iter()
        .map(|(column, literal)| format!("{} = {literal}", quoted(column)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn assignments(sets: &[(String, String)]) -> String {
    sets.iter()
        .map(|(column, literal)| format!("{} = {literal}", quoted(column)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The allowlist is the whole gate: an unknown verb counts as mutating.
/// `BEGIN`, `START` and `SET` are not here: `BEGIN READ WRITE` and
/// `SET default_transaction_read_only` would undo the read-only session that
/// backs a read-only posture (architecture.md §4).
const READING: &[&str] = &[
    "ABORT",
    "COMMIT",
    "END",
    "FETCH",
    "RELEASE",
    "ROLLBACK",
    "SAVEPOINT",
    "SELECT",
    "SHOW",
    "TABLE",
    "VALUES",
    "WITH",
];

/// Gated by posture and typed confirmation. `DO` and `CALL` run code that may
/// do anything, so they are confirmed as if they dropped something.
const DESTRUCTIVE: &[&str] = &["CALL", "DO", "DROP", "TRUNCATE"];

const CATALOG: &[&str] = &[
    "ALTER", "ANALYZE", "CLUSTER", "COMMENT", "CREATE", "GRANT", "REFRESH", "REINDEX", "REVOKE",
    "VACUUM",
];

/// A word anywhere in a read that makes it a write: a data-modifying `WITH`,
/// `SELECT … FOR UPDATE`, a function that changes settings.
const WRITING: &[&str] = &["DELETE", "INSERT", "MERGE", "SET_CONFIG", "UPDATE"];

fn kind_of_statement(words: &[String]) -> StatementKind {
    let Some(verb) = words.first().map(String::as_str) else {
        return StatementKind::Write;
    };
    let writes = || words.iter().any(|word| WRITING.contains(&word.as_str()));

    if verb == "EXPLAIN" {
        // `EXPLAIN ANALYZE` runs what it explains; a plain one runs nothing.
        let analyzes = words
            .iter()
            .any(|word| word == "ANALYZE" || word == "ANALYSE");
        return if analyzes && writes() {
            StatementKind::Write
        } else {
            StatementKind::Read
        };
    }

    if READING.contains(&verb) {
        if writes() {
            StatementKind::Write
        } else {
            StatementKind::Read
        }
    } else if DESTRUCTIVE.contains(&verb) {
        StatementKind::Destructive
    } else if CATALOG.contains(&verb) {
        StatementKind::Catalog
    } else {
        // Includes every verb this table has not learned.
        StatementKind::Write
    }
}

/// Least to most restricted.
const fn severity(kind: StatementKind) -> u8 {
    match kind {
        StatementKind::Read => 0,
        StatementKind::Write => 1,
        StatementKind::Catalog => 2,
        StatementKind::Destructive => 3,
    }
}

/// The most restricted statement decides. An empty buffer classifies as
/// `Write`, like an unknown verb.
fn kind_of(sql: &str) -> StatementKind {
    crate::sql::statement_words(sql)
        .iter()
        .map(|words| kind_of_statement(words))
        .max_by_key(|kind| severity(*kind))
        .unwrap_or(StatementKind::Write)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_a_table_quotes_only_what_postgres_would_fold_and_orders_by_its_key() {
        assert_eq!(
            Statement::select_all("shop", "orders", &[]).sql,
            "SELECT * FROM shop.orders"
        );
        assert_eq!(
            Statement::select_all(
                "Edge Cases",
                "Mixed Case",
                &["Id".to_owned(), "line".to_owned()]
            )
            .sql,
            "SELECT * FROM \"Edge Cases\".\"Mixed Case\" ORDER BY \"Id\", line"
        );
    }

    #[test]
    fn a_name_that_is_not_all_lower_case_survives_the_round_trip() {
        // Unquoted, Postgres would fold this to `myks`.
        assert_eq!(
            Statement::select_all("MyKs", "Orders", &[]).sql,
            "SELECT * FROM \"MyKs\".\"Orders\""
        );
    }

    #[test]
    fn a_quote_inside_a_name_is_doubled_rather_than_ending_the_identifier() {
        assert_eq!(quoted("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn selecting_is_a_read_and_so_survives_a_read_only_connection() {
        assert!(!Statement::select_all("shop", "orders", &[]).mutates());
    }

    #[test]
    fn selecting_is_a_read() {
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders").kind,
            StatementKind::Read
        );
    }

    #[test]
    fn case_and_leading_space_do_not_change_a_kind() {
        assert_eq!(Statement::typed("  select 1").kind, StatementKind::Read);
        assert_eq!(
            Statement::typed("InSeRt INTO t (a) VALUES (1)").kind,
            StatementKind::Write
        );
    }

    #[test]
    fn dropping_and_truncating_are_destructive() {
        for sql in ["DROP TABLE shop.orders", "truncate shop.orders"] {
            assert_eq!(
                Statement::typed(sql).kind,
                StatementKind::Destructive,
                "{sql} must not reach the server without confirmation"
            );
        }
    }

    #[test]
    fn creating_and_altering_are_schema_changes() {
        assert_eq!(
            Statement::typed("CREATE TABLE t (a int PRIMARY KEY)").kind,
            StatementKind::Catalog
        );
        assert_eq!(
            Statement::typed("ALTER TABLE t ADD b int").kind,
            StatementKind::Catalog
        );
    }

    #[test]
    fn an_unknown_verb_is_treated_as_mutating() {
        assert!(Statement::typed("REBUILD INDEX shop.orders").mutates());
        assert!(Statement::typed("").mutates());
    }

    #[test]
    fn a_leading_comment_does_not_hide_the_verb() {
        for sql in [
            "-- what this does\nSELECT * FROM shop.orders",
            "/* what /* nested */ this does */ SELECT * FROM shop.orders",
        ] {
            assert_eq!(Statement::typed(sql).kind, StatementKind::Read, "{sql}");
        }
    }

    #[test]
    fn a_query_and_ending_a_transaction_is_not_a_mutation() {
        for sql in [
            "VALUES (1), (2)",
            "TABLE shop.orders",
            "SHOW search_path",
            "WITH recent AS (SELECT 1) SELECT * FROM recent",
            "EXPLAIN SELECT * FROM shop.orders",
            "EXPLAIN (ANALYZE, BUFFERS) SELECT * FROM shop.orders",
            "COMMIT",
            "ROLLBACK",
        ] {
            assert!(
                !Statement::typed(sql).mutates(),
                "{sql} changes nothing and a read-only connection may run it"
            );
        }
    }

    #[test]
    fn what_could_lift_a_read_only_session_is_a_write() {
        for sql in [
            "BEGIN READ WRITE",
            "BEGIN",
            "START TRANSACTION READ WRITE",
            "SET default_transaction_read_only = off",
            "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE",
            "RESET ALL",
            "SELECT set_config('default_transaction_read_only', 'off', false)",
        ] {
            assert_eq!(Statement::typed(sql).kind, StatementKind::Write, "{sql}");
        }
    }

    #[test]
    fn a_read_that_writes_on_the_way_is_a_write() {
        for sql in [
            "WITH gone AS (DELETE FROM shop.orders RETURNING *) SELECT count(*) FROM gone",
            "SELECT * FROM shop.orders FOR UPDATE",
            "EXPLAIN ANALYZE UPDATE shop.orders SET total = 0",
            "EXPLAIN (ANALYZE) INSERT INTO shop.orders VALUES (1)",
        ] {
            assert_eq!(Statement::typed(sql).kind, StatementKind::Write, "{sql}");
        }
        assert_eq!(
            Statement::typed("EXPLAIN UPDATE shop.orders SET total = 0").kind,
            StatementKind::Read,
            "without ANALYZE the update is only planned"
        );
    }

    #[test]
    fn a_dollar_quoted_body_hides_no_statement_from_the_classifier() {
        assert_eq!(
            Statement::typed("SELECT $$ ; DROP TABLE x $$").kind,
            StatementKind::Read,
            "the body is a string, not a second statement"
        );
        assert_eq!(
            Statement::typed("DO $$ BEGIN DELETE FROM t; END $$").kind,
            StatementKind::Destructive,
            "a block runs code, so it is confirmed"
        );
    }

    #[test]
    fn every_verb_that_writes_rows_is_a_write() {
        for sql in [
            "INSERT INTO shop.orders (id) VALUES (1)",
            "UPDATE shop.orders SET total = 1 WHERE id = 1",
            "DELETE FROM shop.orders WHERE id = 1",
            "MERGE INTO shop.orders USING src ON true WHEN MATCHED THEN DELETE",
            "COPY shop.orders FROM STDIN",
            "LOCK TABLE shop.orders",
        ] {
            assert_eq!(Statement::typed(sql).kind, StatementKind::Write, "{sql}");
        }
    }

    #[test]
    fn every_verb_that_changes_schema_or_permissions_is_a_schema_change() {
        for sql in [
            "CREATE TABLE shop.orders (id int PRIMARY KEY)",
            "CREATE SCHEMA shop",
            "CREATE INDEX ON shop.orders (total)",
            "CREATE MATERIALIZED VIEW shop.v AS SELECT * FROM shop.orders",
            "CREATE ROLE reader",
            "ALTER TABLE shop.orders ADD note text",
            "COMMENT ON TABLE shop.orders IS 'x'",
            "GRANT SELECT ON ALL TABLES IN SCHEMA shop TO reader",
            "REVOKE SELECT ON shop.orders FROM reader",
            "VACUUM shop.orders",
            "REFRESH MATERIALIZED VIEW shop.v",
        ] {
            assert_eq!(Statement::typed(sql).kind, StatementKind::Catalog, "{sql}");
        }
    }

    #[test]
    fn every_verb_that_removes_something_is_destructive() {
        for sql in [
            "DROP TABLE shop.orders",
            "DROP SCHEMA shop CASCADE",
            "DROP INDEX shop.orders_total",
            "DROP MATERIALIZED VIEW shop.v",
            "DROP TYPE shop.address",
            "DROP ROLE reader",
            "TRUNCATE shop.orders",
            "TRUNCATE TABLE shop.orders",
            "CALL shop.purge()",
        ] {
            assert_eq!(
                Statement::typed(sql).kind,
                StatementKind::Destructive,
                "{sql} must not reach the server without confirmation"
            );
        }
    }

    #[test]
    fn a_buffer_takes_the_most_restricted_kind_in_it() {
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders; DROP TABLE shop.orders").kind,
            StatementKind::Destructive
        );
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders; UPDATE shop.orders SET a = 1").kind,
            StatementKind::Write
        );
        assert_eq!(
            Statement::typed("CREATE TABLE t (a int PRIMARY KEY); SELECT * FROM t").kind,
            StatementKind::Catalog
        );
    }

    #[test]
    fn a_semicolon_inside_a_string_does_not_open_a_statement() {
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders WHERE note = '; DROP TABLE x'").kind,
            StatementKind::Read
        );
    }

    #[test]
    fn a_trailing_semicolon_adds_no_statement() {
        assert_eq!(
            Statement::typed("SELECT * FROM shop.orders;\n").kind,
            StatementKind::Read
        );
    }

    #[test]
    fn the_cql_is_kept_verbatim() {
        let sql = "SELECT *\nFROM shop.orders";
        assert_eq!(Statement::typed(sql).sql, sql);
    }

    #[test]
    fn an_insert_quotes_the_names_and_is_a_write() {
        let stmt = Statement::insert_row(
            "My\"Ks",
            "orders",
            &[
                ("id".to_owned(), "7".to_owned()),
                ("note".to_owned(), "'x'".to_owned()),
            ],
        );

        assert_eq!(
            stmt.sql,
            "INSERT INTO \"My\"\"Ks\".\"orders\" (\"id\", \"note\") VALUES (7, 'x')"
        );
        assert_eq!(stmt.kind, StatementKind::Write);
        assert!(stmt.mutates());
    }

    #[test]
    fn dropping_quotes_the_names_and_is_destructive() {
        let table = Statement::drop_table("MyKs", "Orders");
        assert_eq!(table.sql, "DROP TABLE \"MyKs\".\"Orders\"");
        assert_eq!(table.kind, StatementKind::Destructive);

        let keyspace = Statement::drop_keyspace("MyKs");
        assert_eq!(keyspace.sql, "DROP KEYSPACE \"MyKs\"");
        assert_eq!(keyspace.kind, StatementKind::Destructive);
    }

    #[test]
    fn a_built_statement_classifies_as_its_own_cql_would() {
        for built in [
            Statement::drop_table("shop", "orders"),
            Statement::drop_keyspace("shop"),
            Statement::delete_row("shop", "orders", &key()),
            Statement::update_row("shop", "orders", &sets(), &key()),
        ] {
            assert_eq!(
                built.kind,
                Statement::typed(&built.sql).kind,
                "{} classifies two ways",
                built.sql
            );
        }
    }

    fn key() -> Vec<(String, String)> {
        vec![("id".to_owned(), "8f14e45f".to_owned())]
    }

    fn sets() -> Vec<(String, String)> {
        vec![("note".to_owned(), "'x'".to_owned())]
    }

    #[test]
    fn updating_a_row_sets_in_the_order_given_and_quotes_every_name() {
        assert_eq!(
            Statement::update_row(
                "shop",
                "orders",
                &[
                    ("note".to_owned(), "'o''hara'".to_owned()),
                    ("qty".to_owned(), "7".to_owned()),
                ],
                &key()
            )
            .sql,
            "UPDATE \"shop\".\"orders\" SET \"note\" = 'o''hara', \"qty\" = 7 \
             WHERE \"id\" = 8f14e45f"
        );
    }

    #[test]
    fn one_updated_row_is_a_statement_and_several_are_a_batch() {
        assert_eq!(
            Statement::update_rows("shop", "orders", &[(sets(), key())]).sql,
            Statement::update_row("shop", "orders", &sets(), &key()).sql
        );

        let two = vec![
            (sets(), key()),
            (
                vec![("qty".to_owned(), "7".to_owned())],
                vec![("id".to_owned(), "c9f0f895".to_owned())],
            ),
        ];
        assert_eq!(
            Statement::update_rows("shop", "orders", &two).sql,
            "BEGIN;\n\
             UPDATE \"shop\".\"orders\" SET \"note\" = 'x' WHERE \"id\" = 8f14e45f;\n\
             UPDATE \"shop\".\"orders\" SET \"qty\" = 7 WHERE \"id\" = c9f0f895;\n\
             COMMIT;"
        );
    }

    #[test]
    fn an_update_is_a_write_a_read_only_connection_still_refuses() {
        let stmt = Statement::update_row("shop", "orders", &sets(), &key());

        assert_eq!(stmt.kind, StatementKind::Write);
        assert!(stmt.mutates());
    }

    #[test]
    fn deleting_a_row_restricts_every_key_column_and_quotes_the_names() {
        assert_eq!(
            Statement::delete_row(
                "shop",
                "orders",
                &[
                    ("id".to_owned(), "8f14e45f".to_owned()),
                    ("placed".to_owned(), "'2026-01-01'".to_owned()),
                ]
            )
            .sql,
            "DELETE FROM \"shop\".\"orders\" WHERE \"id\" = 8f14e45f AND \"placed\" = '2026-01-01'"
        );
    }

    #[test]
    fn a_delete_is_a_write_rather_than_a_destructive_statement() {
        let stmt = Statement::delete_row("shop", "orders", &key());

        assert_eq!(stmt.kind, StatementKind::Write);
        assert!(stmt.mutates(), "a read-only connection still refuses it");
    }

    #[test]
    fn one_key_is_a_statement_and_several_are_a_batch() {
        assert_eq!(
            Statement::delete_rows("shop", "orders", &[key()]).sql,
            Statement::delete_row("shop", "orders", &key()).sql,
            "a batch of one is a batch the user has to read past"
        );

        let two = vec![key(), vec![("id".to_owned(), "c9f0f895".to_owned())]];
        assert_eq!(
            Statement::delete_rows("shop", "orders", &two).sql,
            "BEGIN;\n\
             DELETE FROM \"shop\".\"orders\" WHERE \"id\" = 8f14e45f;\n\
             DELETE FROM \"shop\".\"orders\" WHERE \"id\" = c9f0f895;\n\
             COMMIT;"
        );
    }
}
