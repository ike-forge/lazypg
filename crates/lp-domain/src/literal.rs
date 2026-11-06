//! Spelling a value back into a Postgres literal. One form fits every type:
//! the text Postgres printed, quoted and cast to the declared type
//! (`'8f14…'::uuid`, `'\x0a'::bytea`, `'{1,2}'::integer[]`), so Postgres parses
//! it back with its own input function. Integers and booleans are written bare.

use crate::model::Value;

/// A value that addresses a row, for a primary key's `WHERE`. `None` where a
/// re-spelling may not compare equal: a float printed and read back can
/// differ, and no key holds `NULL`.
#[must_use]
pub fn key(data_type: &str, value: &Value) -> Option<String> {
    match value {
        Value::Null | Value::Float(_) => None,
        other => Some(of(data_type, other)),
    }
}

/// Any value, for a `SET` or a `VALUES` list.
#[must_use]
pub fn of(data_type: &str, value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::Int(number) => number.to_string(),
        Value::Boolean(flag) => flag.to_string(),
        other => cast(&crate::cell::text(other), data_type),
    }
}

/// `standard_conforming_strings` is on by default since 9.1, so a backslash
/// is literal and only the quote needs doubling.
#[must_use]
pub fn cast(text: &str, data_type: &str) -> String {
    format!("{}::{data_type}", quoted(text))
}

#[must_use]
pub fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_quoted_cast_and_an_embedded_quote_doubled() {
        assert_eq!(
            of("text", &Value::Text("o'hara".to_owned())),
            "'o''hara'::text",
            "a name holding a quote must not end the literal"
        );
    }

    #[test]
    fn every_other_type_is_its_text_cast_to_the_declared_type() {
        assert_eq!(
            of("uuid", &Value::Uuid("8f14e45f-0000".to_owned())),
            "'8f14e45f-0000'::uuid"
        );
        assert_eq!(
            of("bytea", &Value::Blob(vec![0xde, 0xad])),
            "'\\xdead'::bytea"
        );
        assert_eq!(
            of("integer[]", &Value::Array("{1,2}".to_owned())),
            "'{1,2}'::integer[]"
        );
        assert_eq!(
            of("numeric(10,2)", &Value::Numeric("1.50".to_owned())),
            "'1.50'::numeric(10,2)",
            "the digits as Postgres printed them"
        );
        assert_eq!(
            of(
                "timestamp with time zone",
                &Value::Text("2026-01-01 10:00:00+00".to_owned())
            ),
            "'2026-01-01 10:00:00+00'::timestamp with time zone"
        );
    }

    #[test]
    fn integers_and_booleans_are_bare_and_null_is_null() {
        assert_eq!(of("bigint", &Value::Int(-7)), "-7");
        assert_eq!(of("boolean", &Value::Boolean(true)), "true");
        assert_eq!(of("text", &Value::Null), "NULL");
    }

    #[test]
    fn a_key_refuses_what_may_not_compare_equal_when_read_back() {
        assert_eq!(key("double precision", &Value::Float(0.1)), None);
        assert_eq!(key("integer", &Value::Null), None);
        assert_eq!(key("integer", &Value::Int(3)).as_deref(), Some("3"));
    }
}
