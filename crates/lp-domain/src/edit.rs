//! Typing a value back, and the uncommitted set holding it until `ctrl-s`.
//! Postgres parses what is typed, through [`crate::literal`]'s cast; only what
//! the grid must draw differently is read here, so a typo in an integer or a
//! `bytea` is caught before it is sent.

use std::collections::BTreeMap;

use crate::line::Line;
use crate::literal;
use crate::model::Value;

/// [`parse`] is the only constructor, so the drawn value and the literal cannot
/// drift.
#[derive(Debug, Clone, PartialEq)]
pub struct Edited {
    value: Value,
    literal: String,
}

impl Edited {
    /// Also what a clean cell is compared against.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    #[must_use]
    pub fn literal(&self) -> &str {
        &self.literal
    }
}

/// What a declared type is, as `format_type` spells it, for what typing into
/// it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Integer,
    Float,
    Numeric,
    Boolean,
    Bytes,
    Uuid,
    Json,
    Array,
    /// Text a blank field is the empty string of, not `NULL`.
    Text,
    Other,
}

fn family(data_type: &str) -> Family {
    let declared = data_type.trim().to_ascii_lowercase();
    if declared.ends_with("[]") {
        return Family::Array;
    }
    // `character varying(16)`, `numeric(10,2)`: the modifier changes nothing
    // here.
    let base = declared.split('(').next().unwrap_or_default().trim();

    match base {
        "smallint" | "integer" | "bigint" | "int2" | "int4" | "int8" | "oid" => Family::Integer,
        "real" | "double precision" | "float4" | "float8" => Family::Float,
        "numeric" | "decimal" => Family::Numeric,
        "boolean" | "bool" => Family::Boolean,
        "bytea" => Family::Bytes,
        "uuid" => Family::Uuid,
        "json" | "jsonb" => Family::Json,
        "text" | "character varying" | "varchar" | "character" | "char" | "bpchar" | "name"
        | "citext" => Family::Text,
        _ => Family::Other,
    }
}

/// Every type takes typed text; Postgres is the judge of the rest.
#[must_use]
pub fn editable(_data_type: &str) -> bool {
    true
}

/// A blank field is the empty string for text; for anything else it is
/// `NULL`, which [`parse`] returns.
#[must_use]
pub fn takes_empty(data_type: &str) -> bool {
    family(data_type) == Family::Text
}

/// `None` where the text cannot be the type: the field says so before
/// anything is sent.
#[must_use]
pub fn parse(data_type: &str, text: &str) -> Option<Edited> {
    let family = family(data_type);
    let trimmed = text.trim();

    if trimmed.is_empty() && family != Family::Text {
        return Some(Edited {
            value: Value::Null,
            literal: "NULL".to_owned(),
        });
    }

    let value = match family {
        Family::Integer => Value::Int(trimmed.parse().ok()?),
        Family::Float => Value::Float(float(trimmed)?),
        Family::Numeric => {
            trimmed.parse::<f64>().ok()?;
            Value::Numeric(trimmed.to_owned())
        }
        Family::Boolean => Value::Boolean(boolean(trimmed)?),
        Family::Bytes => Value::Blob(hex(trimmed)?),
        Family::Uuid => {
            let uuid = trimmed.to_ascii_lowercase();
            is_uuid(&uuid).then_some(Value::Uuid(uuid))?
        }
        Family::Json => Value::Json(trimmed.to_owned()),
        Family::Array => Value::Array(trimmed.to_owned()),
        Family::Text => Value::Text(text.to_owned()),
        Family::Other => Value::Text(trimmed.to_owned()),
    };
    let literal = match (&value, family) {
        // As typed: an `f64` round trip would rewrite `1.50` and `1e3`.
        (Value::Float(_), _) => literal::cast(trimmed, data_type),
        (value, _) => literal::of(data_type, value),
    };

    Some(Edited { value, literal })
}

/// Postgres's own spellings, `NaN` and the infinities included.
fn float(text: &str) -> Option<f64> {
    match text.to_ascii_lowercase().as_str() {
        "nan" => Some(f64::NAN),
        "infinity" | "inf" => Some(f64::INFINITY),
        "-infinity" | "-inf" => Some(f64::NEG_INFINITY),
        other => other.parse().ok(),
    }
}

/// Every spelling `boolin` takes.
fn boolean(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "t" | "true" | "y" | "yes" | "on" | "1" => Some(true),
        "f" | "false" | "n" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// 8-4-4-4-12; Postgres takes other spellings, the grid prints this one.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();

    groups.len() == 5
        && groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|group| group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// The `\x` form the grid draws; `\x` alone is the empty value.
fn hex(text: &str) -> Option<Vec<u8>> {
    let digits = text
        .strip_prefix("\\x")
        .or_else(|| text.strip_prefix("\\X"))?;

    if digits.len() % 2 != 0 {
        return None;
    }

    digits
        .as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommitScope {
    #[default]
    All,
    Row(usize),
}

impl CommitScope {
    #[must_use]
    pub const fn admits(self, row: usize) -> bool {
        match self {
            Self::All => true,
            Self::Row(only) => only == row,
        }
    }
}

/// Keyed by page row, not drawn position, so a sort or filter cannot move an
/// edit to another row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Edits(BTreeMap<(usize, usize), Edited>);

impl Edits {
    /// `None` clears, so an edit back to the original leaves no mark.
    pub fn set(&mut self, row: usize, column: usize, edited: Option<Edited>) {
        match edited {
            Some(edited) => {
                self.0.insert((row, column), edited);
            }
            None => {
                self.0.remove(&(row, column));
            }
        }
    }

    #[must_use]
    pub fn get(&self, row: usize, column: usize) -> Option<&Edited> {
        self.0.get(&(row, column))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn cells(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn rows(&self) -> usize {
        self.by_row().count()
    }

    /// `(row, column)` key order already groups by row in column order.
    pub fn by_row(&self) -> impl Iterator<Item = (usize, Vec<(usize, &Edited)>)> {
        let mut grouped: Vec<(usize, Vec<(usize, &Edited)>)> = Vec::new();

        for ((row, column), edited) in &self.0 {
            match grouped.last_mut() {
                Some((at, columns)) if at == row => columns.push((*column, edited)),
                _ => grouped.push((*row, vec![(*column, edited)])),
            }
        }

        grouped.into_iter()
    }

    #[must_use]
    pub fn cells_in(&self, scope: CommitScope) -> usize {
        self.0.keys().filter(|(row, _)| scope.admits(*row)).count()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// The rest stay dirty.
    pub fn drain_in(
        &mut self,
        scope: CommitScope,
    ) -> impl Iterator<Item = ((usize, usize), Edited)> {
        let (taken, kept): (BTreeMap<_, _>, BTreeMap<_, _>) = std::mem::take(&mut self.0)
            .into_iter()
            .partition(|((row, _), _)| scope.admits(*row));
        self.0 = kept;
        taken.into_iter()
    }
}

/// The type is captured: the overlay claims every key, so the schema cannot
/// change underneath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    column: usize,
    name: String,
    data_type: String,
    line: Line,
    /// Tells a field typed to empty from one never touched.
    touched: bool,
}

impl Field {
    /// Seeded with the cell's text, so an immediate `⏎` is a no-op.
    #[must_use]
    pub fn opened(column: usize, name: &str, data_type: &str, text: String) -> Self {
        Self {
            column,
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            line: Line::new(text),
            touched: false,
        }
    }

    #[must_use]
    pub const fn touched(&self) -> bool {
        self.touched
    }

    pub const fn touch(&mut self) {
        self.touched = true;
    }

    #[must_use]
    pub const fn column(&self) -> usize {
        self.column
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn data_type(&self) -> &str {
        &self.data_type
    }

    #[must_use]
    pub const fn line(&self) -> &Line {
        &self.line
    }

    pub const fn line_mut(&mut self) -> &mut Line {
        &mut self.line
    }

    #[must_use]
    pub fn parsed(&self) -> Option<Edited> {
        parse(&self.data_type, self.line.text())
    }

    #[must_use]
    pub fn error(&self) -> Option<String> {
        self.parsed()
            .is_none()
            .then(|| format!("Not a valid {}.", self.data_type))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(data_type: &str, text: &str) -> Option<(Value, String)> {
        parse(data_type, text).map(|edited| (edited.value, edited.literal))
    }

    #[test]
    fn text_is_quoted_cast_and_kept_verbatim_spaces_and_all() {
        assert_eq!(
            parsed("character varying(16)", " o'hara "),
            Some((
                Value::Text(" o'hara ".to_owned()),
                "' o''hara '::character varying(16)".to_owned()
            ))
        );
        assert_eq!(
            parsed("text", ""),
            Some((Value::Text(String::new()), "''::text".to_owned())),
            "a blank text field is the empty string"
        );
    }

    #[test]
    fn a_blank_field_of_any_other_type_is_null() {
        for data_type in [
            "integer",
            "uuid",
            "timestamp with time zone",
            "jsonb",
            "integer[]",
        ] {
            assert_eq!(
                parsed(data_type, "  "),
                Some((Value::Null, "NULL".to_owned())),
                "{data_type}"
            );
        }
    }

    #[test]
    fn an_integer_is_bare_and_a_non_integer_is_refused() {
        assert_eq!(
            parsed("bigint", " 42 "),
            Some((Value::Int(42), "42".to_owned()))
        );
        assert_eq!(parsed("integer", "4.2"), None);
    }

    #[test]
    fn a_float_and_a_numeric_keep_the_digits_typed() {
        assert_eq!(
            parsed("double precision", "1.50"),
            Some((Value::Float(1.5), "'1.50'::double precision".to_owned()))
        );
        assert!(matches!(
            parsed("real", "NaN"),
            Some((Value::Float(f), _)) if f.is_nan()
        ));
        assert_eq!(
            parsed("numeric(10,2)", "3.14159265358979"),
            Some((
                Value::Numeric("3.14159265358979".to_owned()),
                "'3.14159265358979'::numeric(10,2)".to_owned()
            ))
        );
        assert_eq!(parsed("numeric", "pi"), None);
    }

    #[test]
    fn a_boolean_takes_every_spelling_postgres_does() {
        for (typed, flag) in [
            ("t", true),
            ("YES", true),
            ("on", true),
            ("0", false),
            ("off", false),
        ] {
            assert_eq!(
                parsed("boolean", typed),
                Some((Value::Boolean(flag), flag.to_string())),
                "{typed}"
            );
        }
        assert_eq!(parsed("boolean", "maybe"), None);
    }

    #[test]
    fn a_uuid_and_bytea_are_checked_and_cast() {
        assert_eq!(
            parsed("uuid", "8F14E45F-CEEA-467E-A3B4-4C5E3B0F6A11"),
            Some((
                Value::Uuid("8f14e45f-ceea-467e-a3b4-4c5e3b0f6a11".to_owned()),
                "'8f14e45f-ceea-467e-a3b4-4c5e3b0f6a11'::uuid".to_owned()
            ))
        );
        assert_eq!(parsed("uuid", "8f14e45f"), None);
        assert_eq!(
            parsed("bytea", "\\x00FF"),
            Some((Value::Blob(vec![0x00, 0xff]), "'\\x00ff'::bytea".to_owned()))
        );
        assert_eq!(parsed("bytea", "00ff"), None, "the marker is not optional");
    }

    #[test]
    fn json_arrays_and_every_other_type_are_postgres_to_judge() {
        assert_eq!(
            parsed("jsonb", r#"{"a": 1}"#),
            Some((
                Value::Json(r#"{"a": 1}"#.to_owned()),
                r#"'{"a": 1}'::jsonb"#.to_owned()
            ))
        );
        assert_eq!(
            parsed("integer[]", "{1,2}"),
            Some((
                Value::Array("{1,2}".to_owned()),
                "'{1,2}'::integer[]".to_owned()
            ))
        );
        assert_eq!(
            parsed("zoo.mood", "happy"),
            Some((
                Value::Text("happy".to_owned()),
                "'happy'::zoo.mood".to_owned()
            ))
        );
    }

    fn edit(text: &str) -> Edited {
        parse("text", text).expect("text takes anything")
    }

    #[test]
    fn setting_none_clears_what_was_there() {
        let mut edits = Edits::default();
        edits.set(1, 2, Some(edit("x")));
        assert_eq!(edits.cells(), 1);

        edits.set(1, 2, None);

        assert!(edits.is_empty());
    }

    #[test]
    fn edits_group_by_row_in_page_order_and_by_column_within_a_row() {
        let mut edits = Edits::default();
        edits.set(9, 3, Some(edit("c")));
        edits.set(2, 5, Some(edit("b")));
        edits.set(9, 1, Some(edit("a")));

        let grouped: Vec<(usize, Vec<usize>)> = edits
            .by_row()
            .map(|(row, columns)| (row, columns.into_iter().map(|(at, _)| at).collect()))
            .collect();

        assert_eq!(grouped, [(2, vec![5]), (9, vec![1, 3])]);
        assert_eq!((edits.cells(), edits.rows()), (3, 2));
    }

    #[test]
    fn a_field_reports_why_it_does_not_parse_and_nothing_when_it_does() {
        let mut field = Field::opened(0, "qty", "integer", "7".to_owned());
        assert_eq!(field.error(), None);

        field.line_mut().insert('x');

        assert_eq!(field.error().as_deref(), Some("Not a valid integer."));
        assert_eq!(field.parsed(), None);
    }
}
