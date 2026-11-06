//! Laying a statement out without parsing it: whitespace is rebuilt and
//! keywords upper-cased, nothing reordered or dropped, so a misread buffer
//! comes back oddly laid out rather than changed.

use super::keywords;
use super::lex::{Kind, Piece, pieces};

/// Two: the panel is short and narrow.
const INDENT: &str = "  ";

/// Idempotent.
#[must_use]
pub fn format(text: &str) -> String {
    let mut out = Layout::default();

    for piece in pieces(text).into_iter().filter(is_written) {
        out.push(&piece);
    }

    out.finish()
}

fn is_written(piece: &Piece) -> bool {
    piece.kind != Kind::Whitespace
}

#[derive(Default)]
struct Layout {
    lines: Vec<String>,
    line: String,
    /// A clause keyword inside brackets is not a clause.
    depth: usize,
    /// A blank line between statements.
    separate: bool,
    previous: Option<Piece>,
}

impl Layout {
    fn push(&mut self, piece: &Piece) {
        // Rules read the piece as written: `from` opens a clause as `FROM`
        // does.
        let piece = &Piece {
            kind: piece.kind,
            text: self.written(piece),
        };

        if std::mem::take(&mut self.separate) {
            self.lines.push(String::new());
        }

        if self.breaks_before(piece) {
            self.wrap(continues(&piece.text));
        }

        if self.spaces_before(piece) {
            self.line.push(' ');
        }

        self.line.push_str(&piece.text);
        self.track(piece);

        if ends_a_line(piece) {
            self.wrap(false);
        }

        self.separate = piece.text == ";";
    }

    /// A name's case is load-bearing, and a literal's case is its value.
    /// `key` is a name to the lexer, since columns are called that, but after
    /// `PRIMARY` or `FOREIGN` it is grammar.
    fn written(&self, piece: &Piece) -> String {
        let after_key_word = self
            .previous
            .as_ref()
            .is_some_and(|previous| matches!(previous.text.as_str(), "PRIMARY" | "FOREIGN"));

        if piece.kind == Kind::Keyword || (after_key_word && piece.text.eq_ignore_ascii_case("key"))
        {
            piece.text.to_ascii_uppercase()
        } else {
            piece.text.clone()
        }
    }

    /// A line of only [`INDENT`] is still blank.
    fn blank(&self) -> bool {
        self.line.trim_start().is_empty()
    }

    /// Compound clauses (`INSERT INTO`, `LEFT JOIN`) stay whole.
    fn breaks_before(&self, piece: &Piece) -> bool {
        if self.blank() || self.depth > 0 || piece.kind != Kind::Keyword {
            return false;
        }

        let compound = self.previous.as_ref().is_some_and(|previous| {
            previous.kind == Kind::Keyword
                && COMPOUND.contains(&(previous.text.as_str(), piece.text.as_str()))
        });

        !compound && is_clause(&piece.text)
    }

    fn spaces_before(&self, piece: &Piece) -> bool {
        if self.blank() {
            return false;
        }

        let Some(previous) = self.previous.as_ref() else {
            return false;
        };

        if piece.kind == Kind::Comment {
            return true;
        }

        if tight_after(&previous.text) || tight_before(&piece.text) {
            return false;
        }

        // A type's modifier binds as a call's arguments do: `numeric(10, 2)`.
        let typed = previous.kind == Kind::Keyword && keywords::is_type(&previous.text);

        !(piece.text == "("
            && (typed || (previous.kind == Kind::Identifier && self.binds_brackets())))
    }

    /// `count(id)` and `INSERT INTO orders (id)` are the same pieces; the
    /// line's opening clause tells them apart.
    fn binds_brackets(&self) -> bool {
        !DEFINES
            .iter()
            .any(|clause| self.line.trim_start().starts_with(clause))
    }

    fn track(&mut self, piece: &Piece) {
        match piece.text.as_str() {
            "(" | "[" | "{" => self.depth += 1,
            ")" | "]" | "}" => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }

        self.previous = Some(piece.clone());
    }

    /// A blank line is not closed, so consecutive clause keywords leave no gap.
    fn wrap(&mut self, indented: bool) {
        if self.blank() {
            self.line.clear();
            return;
        }

        self.lines.push(std::mem::take(&mut self.line));
        if indented {
            self.line.push_str(INDENT);
        }
    }

    fn finish(mut self) -> String {
        self.wrap(false);

        self.lines.join("\n")
    }
}

/// Only pairs whose second word would open a clause alone.
const COMPOUND: &[(&str, &str)] = &[
    ("CREATE", "OR"),
    ("CROSS", "JOIN"),
    ("DELETE", "FROM"),
    ("DO", "UPDATE"),
    ("EXCEPT", "ALL"),
    ("FOR", "UPDATE"),
    ("FULL", "JOIN"),
    ("FULL", "OUTER"),
    ("INNER", "JOIN"),
    ("INSERT", "INTO"),
    ("INTERSECT", "ALL"),
    ("LEFT", "JOIN"),
    ("LEFT", "OUTER"),
    ("NATURAL", "JOIN"),
    ("NATURAL", "LEFT"),
    ("OUTER", "JOIN"),
    ("RIGHT", "JOIN"),
    ("RIGHT", "OUTER"),
    ("UNION", "ALL"),
];

const DEFINES: &[&str] = &["ALTER", "CREATE", "DROP", "INSERT"];

fn continues(text: &str) -> bool {
    matches!(text, "AND" | "OR")
}

/// Anything after a line comment would be inside it.
fn ends_a_line(piece: &Piece) -> bool {
    piece.text == ";" || (piece.kind == Kind::Comment && !piece.text.starts_with("/*"))
}

/// `:` for the `::` of a cast.
fn tight_after(text: &str) -> bool {
    matches!(text, "(" | "[" | "{" | "." | ":")
}

/// `[` for a subscript, an array type and `ARRAY[…]`.
fn tight_before(text: &str) -> bool {
    matches!(text, ")" | "[" | "]" | "}" | "." | "," | ";" | ":")
}

/// Its own list: type names like `TEXT` are keywords that must never break a
/// line. `ON` stays with its join, `IF` with its `NOT EXISTS`.
fn is_clause(text: &str) -> bool {
    const CLAUSES: &[&str] = &[
        "ALTER",
        "AND",
        "CREATE",
        "CROSS",
        "DELETE",
        "DROP",
        "EXCEPT",
        "FROM",
        "FULL",
        "GRANT",
        "GROUP",
        "HAVING",
        "INNER",
        "INSERT",
        "INTERSECT",
        "JOIN",
        "LEFT",
        "LIMIT",
        "NATURAL",
        "OFFSET",
        "OR",
        "ORDER",
        "RETURNING",
        "REVOKE",
        "RIGHT",
        "SELECT",
        "SET",
        "TRUNCATE",
        "UNION",
        "UPDATE",
        "USING",
        "VALUES",
        "WHERE",
        "WINDOW",
        "WITH",
    ];

    debug_assert!(
        CLAUSES.iter().all(|clause| keywords::is_keyword(clause)),
        "a clause opener that is not a keyword can never be reached"
    );

    CLAUSES.contains(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserted twice, since formatting is idempotent.
    fn formatted(source: &str) -> String {
        let once = format(source);
        assert_eq!(format(&once), once, "formatting is idempotent");

        once
    }

    #[test]
    fn an_empty_buffer_formats_to_nothing() {
        assert_eq!(formatted(""), "");
        assert_eq!(formatted("   \n  "), "");
    }

    #[test]
    fn a_select_breaks_at_each_clause() {
        assert_eq!(
            formatted("select * from shop.orders where id = 7 limit 10"),
            "SELECT *\nFROM shop.orders\nWHERE id = 7\nLIMIT 10"
        );
    }

    #[test]
    fn a_name_keeps_its_case_and_a_keyword_does_not() {
        assert_eq!(
            formatted("select Id from \"MyKs\".Orders"),
            "SELECT Id\nFROM \"MyKs\".Orders"
        );
    }

    #[test]
    fn a_conjunction_is_indented_under_the_clause_it_continues() {
        assert_eq!(
            formatted("SELECT * FROM t WHERE a = 1 AND b = 2 AND c = 3"),
            "SELECT *\nFROM t\nWHERE a = 1\n  AND b = 2\n  AND c = 3"
        );
    }

    #[test]
    fn a_compound_clause_stays_on_one_line() {
        assert_eq!(
            formatted(
                "select o.id from shop.orders o left outer join shop.customer c on c.id = o.customer_id union all select 1 for update"
            ),
            "SELECT o.id\nFROM shop.orders o\nLEFT OUTER JOIN shop.customer c ON c.id = o.customer_id\nUNION ALL\nSELECT 1 FOR UPDATE"
        );
    }

    #[test]
    fn an_insert_keeps_into_with_its_verb() {
        assert_eq!(
            formatted("insert into shop.orders (id, total) values (1, 2)"),
            "INSERT INTO shop.orders (id, total)\nVALUES (1, 2)"
        );
    }

    #[test]
    fn a_clause_keyword_inside_brackets_is_not_a_clause() {
        assert_eq!(
            formatted("SELECT id FROM t WHERE a IN (1, 2) AND b = token(c)"),
            "SELECT id\nFROM t\nWHERE a IN (1, 2)\n  AND b = token(c)"
        );
    }

    #[test]
    fn a_function_binds_its_brackets_and_a_keyword_does_not() {
        assert_eq!(
            formatted("SELECT count ( id ) FROM t WHERE a IN ( 1 )"),
            "SELECT count(id)\nFROM t\nWHERE a IN (1)"
        );
    }

    #[test]
    fn each_statement_gets_its_own_line() {
        assert_eq!(formatted("SELECT 1; SELECT 2;"), "SELECT 1;\n\nSELECT 2;");
    }

    #[test]
    fn a_trailing_separator_adds_no_blank_line() {
        assert_eq!(formatted("SELECT 1;\n\n\n"), "SELECT 1;");
    }

    #[test]
    fn a_line_comment_closes_its_line_and_keeps_what_follows_off_it() {
        assert_eq!(
            formatted("SELECT 1 -- why\nFROM t"),
            "SELECT 1 -- why\nFROM t"
        );
    }

    #[test]
    fn a_block_comment_stays_where_it_was_written() {
        assert_eq!(
            formatted("SELECT /* all */ * FROM t"),
            "SELECT /* all */ *\nFROM t"
        );
    }

    #[test]
    fn a_string_is_never_retyped() {
        assert_eq!(
            formatted("update t set name = 'From  Where' where id = 1"),
            "UPDATE t\nSET name = 'From  Where'\nWHERE id = 1"
        );
    }

    #[test]
    fn a_create_table_keeps_its_definition_together() {
        assert_eq!(
            formatted(
                "create table if not exists shop.orders (id uuid primary key, total int) with (fillfactor = 70)"
            ),
            "CREATE TABLE IF NOT EXISTS shop.orders (id UUID PRIMARY KEY, total INT)\nWITH (fillfactor = 70)"
        );
    }

    #[test]
    fn already_formatted_text_is_left_alone() {
        let laid_out = "SELECT *\nFROM shop.orders\nWHERE id = 7\n  AND total > 10\nLIMIT 200";

        assert_eq!(formatted(laid_out), laid_out);
    }

    #[test]
    fn a_cast_binds_tight_and_a_dollar_quoted_body_is_never_relaid() {
        assert_eq!(
            formatted("select a :: text from t"),
            "SELECT a::TEXT\nFROM t"
        );
        assert_eq!(
            formatted("do $$ begin  select 1; end $$"),
            "DO $$ begin  select 1; end $$"
        );
    }

    #[test]
    fn a_transaction_is_three_statements() {
        assert_eq!(
            formatted("begin; update t set a = 1 returning a; commit;"),
            "BEGIN;\n\nUPDATE t\nSET a = 1\nRETURNING a;\n\nCOMMIT;"
        );
    }

    #[test]
    fn a_bracket_hugs_what_it_subscripts_or_types() {
        assert_eq!(
            formatted("select a[1], '{x}'::text[] from t"),
            "SELECT a[1], '{x}'::TEXT[]\nFROM t"
        );
    }
}
