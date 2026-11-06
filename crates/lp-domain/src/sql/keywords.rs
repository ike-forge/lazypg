//! Words Postgres reads as grammar. Not its whole keyword list: most of it is
//! unreserved and names columns as often (`type`, `key`, `comment`), and a
//! name read as grammar is neither coloured nor completed as one. Types are
//! here, so they draw as grammar.

/// Upper case and sorted, for bisection and a stable completion order.
pub(crate) const KEYWORDS: &[&str] = &[
    "ABORT",
    "ADD",
    "ALL",
    "ALTER",
    "ANALYZE",
    "AND",
    "ANY",
    "ARRAY",
    "AS",
    "ASC",
    "ASYMMETRIC",
    "AUTHORIZATION",
    "BEGIN",
    "BETWEEN",
    "BIGINT",
    "BIGSERIAL",
    "BOOL",
    "BOOLEAN",
    "BOTH",
    "BY",
    "BYTEA",
    "CALL",
    "CASCADE",
    "CASE",
    "CAST",
    "CHAR",
    "CHARACTER",
    "CHECK",
    "CIDR",
    "CLUSTER",
    "COLLATE",
    "COLUMN",
    "COMMIT",
    "CONCURRENTLY",
    "CONFLICT",
    "CONSTRAINT",
    "COPY",
    "CREATE",
    "CROSS",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "CURRENT_TIMESTAMP",
    "CURRENT_USER",
    "CURSOR",
    "DATABASE",
    "DATE",
    "DEALLOCATE",
    "DECIMAL",
    "DECLARE",
    "DEFAULT",
    "DEFERRABLE",
    "DELETE",
    "DESC",
    "DISCARD",
    "DISTINCT",
    "DO",
    "DOUBLE",
    "DROP",
    "ELSE",
    "END",
    "EXCEPT",
    "EXECUTE",
    "EXISTS",
    "EXPLAIN",
    "EXTENSION",
    "FETCH",
    "FLOAT",
    "FLOAT4",
    "FLOAT8",
    "FOR",
    "FOREIGN",
    "FROM",
    "FULL",
    "FUNCTION",
    "GENERATED",
    "GRANT",
    "GROUP",
    "HAVING",
    "IF",
    "ILIKE",
    "IN",
    "INDEX",
    "INET",
    "INHERITS",
    "INNER",
    "INSERT",
    "INT",
    "INT2",
    "INT4",
    "INT8",
    "INTEGER",
    "INTERSECT",
    "INTERVAL",
    "INTO",
    "IS",
    "ISNULL",
    "JOIN",
    "JSON",
    "JSONB",
    "LATERAL",
    "LEADING",
    "LEFT",
    "LIKE",
    "LIMIT",
    "MACADDR",
    "MATERIALIZED",
    "MERGE",
    "MONEY",
    "NATURAL",
    "NOT",
    "NOTNULL",
    "NOWAIT",
    "NULLS",
    "NUMERIC",
    "OFFSET",
    "ON",
    "ONLY",
    "OR",
    "ORDER",
    "OUTER",
    "OVER",
    "OVERLAPS",
    "PRECISION",
    "PREPARE",
    "PRIMARY",
    "PROCEDURE",
    "REAL",
    "RECURSIVE",
    "REFERENCES",
    "REINDEX",
    "RELEASE",
    "RENAME",
    "REPEATABLE",
    "REPLACE",
    "RESET",
    "RESTRICT",
    "RETURNING",
    "RETURNS",
    "REVOKE",
    "RIGHT",
    "ROLLBACK",
    "ROLLUP",
    "SAVEPOINT",
    "SCHEMA",
    "SELECT",
    "SERIAL",
    "SERIALIZABLE",
    "SET",
    "SETOF",
    "SHOW",
    "SIMILAR",
    "SMALLINT",
    "SOME",
    "SYMMETRIC",
    "TABLE",
    "TABLESAMPLE",
    "TEXT",
    "THEN",
    "TIME",
    "TIMESTAMP",
    "TIMESTAMPTZ",
    "TIMETZ",
    "TO",
    "TRAILING",
    "TRANSACTION",
    "TRIGGER",
    "TRUNCATE",
    "TSVECTOR",
    "UNION",
    "UNIQUE",
    "UNLOGGED",
    "UPDATE",
    "USING",
    "UUID",
    "VACUUM",
    "VALUES",
    "VARCHAR",
    "VARYING",
    "VIEW",
    "WHEN",
    "WHERE",
    "WINDOW",
    "WITH",
    "WITHOUT",
    "XML",
];

/// A subset of [`KEYWORDS`], so types stay grammar everywhere; this is what
/// `Expect::Type` offers.
pub(crate) const TYPES: &[&str] = &[
    "BIGINT",
    "BIGSERIAL",
    "BOOL",
    "BOOLEAN",
    "BYTEA",
    "CHAR",
    "CHARACTER",
    "CIDR",
    "DATE",
    "DECIMAL",
    "DOUBLE",
    "FLOAT",
    "FLOAT4",
    "FLOAT8",
    "INET",
    "INT",
    "INT2",
    "INT4",
    "INT8",
    "INTEGER",
    "INTERVAL",
    "JSON",
    "JSONB",
    "MACADDR",
    "MONEY",
    "NUMERIC",
    "REAL",
    "SERIAL",
    "SMALLINT",
    "TEXT",
    "TIME",
    "TIMESTAMP",
    "TIMESTAMPTZ",
    "TIMETZ",
    "TSVECTOR",
    "UUID",
    "VARCHAR",
    "XML",
];

/// The words that continue or end a column list, and nothing else. A union
/// across clauses: splitting per clause would need the resolver to carry which
/// clause it is in.
pub(crate) const COLUMN_WORDS: &[&str] = &[
    "AND",
    "AS",
    "ASC",
    "BETWEEN",
    "BY",
    "DESC",
    "DISTINCT",
    "EXCEPT",
    "FROM",
    "FULL",
    "GROUP",
    "HAVING",
    "ILIKE",
    "IN",
    "INNER",
    "INTERSECT",
    "IS",
    "ISNULL",
    "JOIN",
    "LEFT",
    "LIKE",
    "LIMIT",
    "NOT",
    "NOTNULL",
    "NULLS",
    "OFFSET",
    "ON",
    "OR",
    "ORDER",
    "OVER",
    "RETURNING",
    "RIGHT",
    "SET",
    "UNION",
    "USING",
    "VALUES",
    "WHERE",
    "WINDOW",
];

/// Storage parameters a `WITH (…)` takes. Not keywords: ordinary identifiers
/// elsewhere.
pub(crate) const PROPERTIES: &[&str] = &[
    "autovacuum_enabled",
    "fillfactor",
    "parallel_workers",
    "toast_tuple_target",
];

/// Keywords that mean values, so drawn as literals.
pub(crate) const VALUE_WORDS: &[&str] = &["FALSE", "NULL", "TRUE"];

pub(crate) fn is_keyword(word: &str) -> bool {
    lookup(KEYWORDS, word)
}

pub(crate) fn is_value_word(word: &str) -> bool {
    lookup(VALUE_WORDS, word)
}

fn lookup(table: &[&str], word: &str) -> bool {
    let upper = word.to_ascii_uppercase();

    table.binary_search(&upper.as_str()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_sorted_so_a_lookup_can_bisect() {
        for table in [KEYWORDS, COLUMN_WORDS, VALUE_WORDS, TYPES, PROPERTIES] {
            let mut sorted = table.to_vec();
            sorted.sort_unstable();
            assert_eq!(table, sorted, "the table is walked and bisected in order");
        }
    }

    #[test]
    fn a_keyword_is_recognised_in_any_case() {
        assert!(is_keyword("select"));
        assert!(is_keyword("SELECT"));
        assert!(is_keyword("SeLeCt"));
    }

    #[test]
    fn a_name_is_not_a_keyword() {
        assert!(!is_keyword("orders"));
        assert!(!is_keyword(""));
    }

    #[test]
    fn every_type_is_also_grammar() {
        for word in TYPES {
            assert!(
                is_keyword(word),
                "{word} is offered as a type and must still highlight as one"
            );
        }
    }

    #[test]
    fn every_word_a_column_position_takes_is_grammar_everywhere() {
        for word in COLUMN_WORDS {
            assert!(
                is_keyword(word),
                "{word} is offered in a column list and must still highlight as grammar"
            );
        }
    }

    #[test]
    fn a_property_is_a_name_everywhere_but_its_own_clause() {
        for word in PROPERTIES {
            assert!(
                !is_keyword(word),
                "{word} may be a column name, and colouring it as grammar would lie"
            );
        }
    }

    #[test]
    fn the_value_words_are_not_in_the_keyword_table() {
        for word in VALUE_WORDS {
            assert!(
                !is_keyword(word),
                "{word} is a literal, and one word cannot be both"
            );
            assert!(is_value_word(word));
        }
    }
}
