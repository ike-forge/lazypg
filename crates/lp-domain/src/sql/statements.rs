//! The buffer split into runnable statements: at every `;` the lexer did not
//! read inside a string, a quoted name, a dollar-quoted body or a comment.

use super::lex::{Kind, pieces};

/// Counts `char`s, as `Editor`'s cursor does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use]
    pub fn of(self, text: &str) -> String {
        text.chars().take(self.end).skip(self.start).collect()
    }
}

/// Whitespace and comments between statements fall outside every span. A
/// trailing `;` belongs to its statement.
#[must_use]
pub fn statements(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut at = 0;
    let mut start = None;
    let mut end = 0;

    for piece in pieces(text) {
        let next = at + piece.text.chars().count();

        match piece.kind {
            Kind::Whitespace | Kind::Comment => {}
            Kind::Punctuation if piece.text == ";" => {
                if let Some(start) = start.take() {
                    spans.push(Span { start, end: next });
                }
            }
            _ => {
                start.get_or_insert(at);
                end = next;
            }
        }

        at = next;
    }

    if let Some(start) = start {
        spans.push(Span { start, end });
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(text: &str) -> Vec<String> {
        statements(text)
            .into_iter()
            .map(|span| span.of(text))
            .collect()
    }

    #[test]
    fn an_empty_buffer_holds_no_statement() {
        assert!(statements("").is_empty());
        assert!(statements("   \n\t ").is_empty());
    }

    #[test]
    fn a_buffer_with_no_separator_is_one_statement() {
        assert_eq!(
            split("SELECT * FROM shop.orders"),
            ["SELECT * FROM shop.orders"]
        );
    }

    #[test]
    fn a_semicolon_ends_a_statement_and_stays_with_it() {
        assert_eq!(
            split("SELECT * FROM a; SELECT * FROM b"),
            ["SELECT * FROM a;", "SELECT * FROM b"]
        );
    }

    #[test]
    fn the_whitespace_around_a_statement_falls_outside_it() {
        let text = "\n  SELECT 1;\n\n  SELECT 2\n";

        assert_eq!(split(text), ["SELECT 1;", "SELECT 2"]);
        assert_eq!(statements(text)[0].start, 3);
    }

    #[test]
    fn a_comment_between_statements_belongs_to_neither() {
        assert_eq!(
            split("SELECT 1;\n-- why\nSELECT 2;\n/* done */"),
            ["SELECT 1;", "SELECT 2;"]
        );
    }

    /// A comment before a keyword on the same run is inside the statement.
    #[test]
    fn a_comment_inside_a_statement_is_part_of_it() {
        assert_eq!(
            split("SELECT * -- all\nFROM a"),
            ["SELECT * -- all\nFROM a"]
        );
    }

    #[test]
    fn a_semicolon_inside_a_literal_separates_nothing() {
        assert_eq!(
            split("INSERT INTO a (v) VALUES ('one; two')"),
            ["INSERT INTO a (v) VALUES ('one; two')"]
        );
    }

    #[test]
    fn a_stray_semicolon_yields_no_empty_statement() {
        assert!(statements(";").is_empty());
        assert_eq!(split(";;SELECT 1;;"), ["SELECT 1;"]);
    }

    #[test]
    fn a_multibyte_character_counts_as_one() {
        let text = "SELECT 'café';SELECT 1";

        assert_eq!(split(text), ["SELECT 'café';", "SELECT 1"]);
        assert_eq!(statements(text)[1].start, 14);
    }

    #[test]
    fn a_transaction_is_each_of_its_statements() {
        assert_eq!(
            split("BEGIN; UPDATE t SET a = 1; COMMIT;"),
            ["BEGIN;", "UPDATE t SET a = 1;", "COMMIT;"]
        );
    }

    #[test]
    fn a_semicolon_in_a_function_body_splits_nothing() {
        let create = "CREATE FUNCTION f() RETURNS int AS $body$ BEGIN RETURN 1; END $body$ LANGUAGE plpgsql;";
        assert_eq!(
            split(&format!("{create} SELECT f();")),
            [create, "SELECT f();"]
        );
    }
}
