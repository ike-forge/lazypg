//! The cell inspector: one value or one row, read whole. The domain counts
//! laid-out lines; wrapping is the renderer's, so the offset follows the span
//! the last frame reports.

use std::collections::BTreeMap;

use crate::bytes::Bytes;
use crate::cell;
use crate::contents::Subject;
use crate::edit::{Edited, Field};
use crate::model::Value;
use crate::style::Segment;
use crate::viewport::{Scroll, Span};

/// Copied out of the page: the grid can be replaced while the overlay is up.
#[derive(Debug, Clone, PartialEq)]
pub enum Inspected {
    Cell {
        /// The page column index, which edits are keyed by.
        at: usize,
        column: String,
        value: Value,
    },
    Row {
        /// One-based.
        number: usize,
        columns: Vec<String>,
        values: Vec<Value>,
    },
}

impl Inspected {
    fn values(&self) -> &[Value] {
        match self {
            Self::Cell { value, .. } => std::slice::from_ref(value),
            Self::Row { values, .. } => values,
        }
    }

    fn column_of(&self, at: usize) -> Option<(usize, &str)> {
        match self {
            Self::Cell {
                at: index, column, ..
            } if at == 0 => Some((*index, column.as_str())),
            Self::Cell { .. } => None,
            Self::Row { columns, .. } => columns.get(at).map(|name| (at, name.as_str())),
        }
    }

    /// The inspector holds a copy of the page, which must follow the edit.
    fn set(&mut self, at: usize, value: Value) {
        match self {
            Self::Cell { value: held, .. } if at == 0 => *held = value,
            Self::Cell { .. } => {}
            Self::Row { values, .. } => {
                if let Some(held) = values.get_mut(at) {
                    *held = value;
                }
            }
        }
    }
}

/// The cursor counts laid-out lines (width-independent); the offset counts
/// wrapped lines and follows by span.
#[derive(Debug, Clone, PartialEq)]
pub struct Inspector {
    /// The drawn row an edit made here applies back to.
    row: usize,
    subject: Subject,
    inspected: Inspected,
    cursor: usize,
    scroll: Scroll,
    /// On the inspector, not a sibling overlay: a field with no value under it
    /// is not a state.
    field: Option<Field>,
    /// Indices into the drawn values, so the projection needs no page.
    edited: Vec<usize>,
    /// Set while the block is the form for a row not yet inserted.
    new_row: Option<BTreeMap<usize, Edited>>,
}

impl Inspector {
    #[must_use]
    pub fn opened(row: usize, subject: Subject, inspected: Inspected) -> Self {
        Self {
            row,
            subject,
            inspected,
            cursor: 0,
            scroll: Scroll::default(),
            field: None,
            edited: Vec::new(),
            new_row: None,
        }
    }

    /// The grid row is `0` and means nothing: nothing typed here applies to the
    /// page.
    #[must_use]
    pub fn inserting(subject: Subject, columns: Vec<String>) -> Self {
        let values = vec![Value::Null; columns.len()];
        let mut form = Self::opened(
            0,
            subject,
            Inspected::Row {
                number: 0,
                columns,
                values,
            },
        );
        form.new_row = Some(BTreeMap::new());

        form
    }

    #[must_use]
    pub const fn is_inserting(&self) -> bool {
        self.new_row.is_some()
    }

    /// In column order, which is the statement's order.
    pub fn typed(&self) -> impl Iterator<Item = (usize, &Edited)> {
        self.new_row
            .iter()
            .flat_map(|typed| typed.iter().map(|(at, edited)| (*at, edited)))
    }

    /// An empty field is accepted in the form, where it unsets the column.
    #[must_use]
    pub fn can_accept(&self) -> bool {
        self.field.as_ref().is_some_and(|field| {
            field.parsed().is_some() || (self.is_inserting() && field.line().text().is_empty())
        })
    }

    #[must_use]
    pub fn field_error(&self) -> Option<String> {
        if self.can_accept() {
            return None;
        }

        self.field.as_ref().and_then(Field::error)
    }

    #[must_use]
    pub fn is_set_at_cursor(&self) -> bool {
        self.new_row
            .as_ref()
            .is_some_and(|typed| typed.contains_key(&self.field_at_cursor()))
    }

    /// The only way back to "not written" once a free-text column holds a
    /// value.
    pub fn unset(&mut self) {
        let at = self.field_at_cursor();

        if let Some(typed) = self.new_row.as_mut() {
            typed.remove(&at);
            self.inspected.set(at, Value::Null);
            self.edited.retain(|marked| *marked != at);
        }
    }

    /// `false` while the text does not parse.
    pub fn accept_new(&mut self) -> bool {
        if !self.can_accept() {
            return false;
        }
        let Some(at) = self.at_field().map(|(at, _, _)| at) else {
            return false;
        };
        let Some(field) = self.field.take() else {
            return false;
        };
        let Some(typed) = self.new_row.as_mut() else {
            self.field = Some(field);
            return false;
        };

        // Opening a field and pressing `⏎` leaves the column unset.
        let blank = field.line().text().is_empty();
        let keeps_blank = blank
            && crate::edit::takes_empty(field.data_type())
            && (field.touched() || typed.contains_key(&at));

        if let Some(edited) = field.parsed()
            && (!blank || keeps_blank)
        {
            self.inspected.set(at, edited.value().clone());
            typed.insert(at, edited);
            if !self.edited.contains(&at) {
                self.edited.push(at);
            }
        } else {
            self.inspected.set(at, Value::Null);
            typed.remove(&at);
            self.edited.retain(|marked| *marked != at);
        }

        true
    }

    #[must_use]
    pub const fn row(&self) -> usize {
        self.row
    }

    /// Marks fields already dirty, so a reopened inspector agrees with the
    /// grid.
    #[must_use]
    pub fn with_edited(self, edited: Vec<usize>) -> Self {
        Self { edited, ..self }
    }

    #[must_use]
    pub fn edited(&self) -> &[usize] {
        &self.edited
    }

    #[must_use]
    pub const fn field(&self) -> Option<&Field> {
        self.field.as_ref()
    }

    pub const fn field_mut(&mut self) -> Option<&mut Field> {
        self.field.as_mut()
    }

    /// `None` on a value that belongs to no page column.
    #[must_use]
    pub fn at_field(&self) -> Option<(usize, usize, &str)> {
        let at = self.field_at_cursor();

        self.inspected
            .column_of(at)
            .map(|(column, name)| (at, column, name))
    }

    #[must_use]
    pub fn value_at_cursor(&self) -> Option<&Value> {
        self.inspected.values().get(self.field_at_cursor())
    }

    #[must_use]
    pub fn field_count(&self) -> usize {
        self.inspected.values().len()
    }

    pub fn focus_field(&mut self, at: usize) {
        let before = self
            .inspected
            .values()
            .iter()
            .take(at)
            .map(|value| lines(value).len())
            .sum();

        self.go_to(before);
    }

    pub fn start_edit(&mut self, field: Field) {
        self.field = Some(field);
    }

    pub fn cancel_edit(&mut self) {
        self.field = None;
    }

    /// `None` while the text does not parse, which keeps what was typed.
    pub fn accept_edit(&mut self) -> Option<(usize, usize, Edited)> {
        let edited = self.field.as_ref()?.parsed()?;

        self.accept(edited)
    }

    pub fn accept_null(&mut self) -> Option<(usize, usize, Edited)> {
        self.accept(crate::edit::null())
    }

    fn accept(&mut self, edited: Edited) -> Option<(usize, usize, Edited)> {
        let (at, column, _) = self.at_field()?;

        self.inspected.set(at, edited.value().clone());
        self.field = None;
        if !self.edited.contains(&at) {
            self.edited.push(at);
        }

        Some((at, column, edited))
    }

    pub fn unmark(&mut self, at: usize) {
        self.edited.retain(|marked| *marked != at);
    }

    fn field_at_cursor(&self) -> usize {
        let mut at = self.cursor;

        for (index, value) in self.inspected.values().iter().enumerate() {
            let laid = lines(value).len();

            if at < laid {
                return index;
            }
            at -= laid;
        }

        self.inspected.values().len().saturating_sub(1)
    }

    #[must_use]
    pub const fn subject(&self) -> &Subject {
        &self.subject
    }

    #[must_use]
    pub const fn inspected(&self) -> &Inspected {
        &self.inspected
    }

    #[must_use]
    pub const fn offset(&self) -> usize {
        self.scroll.offset()
    }

    #[must_use]
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.inspected
            .values()
            .iter()
            .map(|value| lines(value).len())
            .sum()
    }

    pub fn down(&mut self) {
        self.go_to(self.cursor.saturating_add(1));
    }

    pub const fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn page_down(&mut self, height: usize) {
        self.go_to(self.cursor.saturating_add(height));
    }

    pub const fn page_up(&mut self, height: usize) {
        self.cursor = self.cursor.saturating_sub(height);
    }

    pub const fn to_start(&mut self) {
        self.cursor = 0;
    }

    pub fn to_end(&mut self) {
        self.go_to(usize::MAX);
    }

    fn go_to(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.line_count().saturating_sub(1));
    }

    pub const fn follow(&mut self, span: Span, height: usize) {
        self.scroll.follow_span(span, height);
    }

    pub const fn clamp_to(&mut self, lines: usize, height: usize) {
        self.scroll.clamp_to(lines, height);
    }

    #[must_use]
    pub fn yank(&self) -> Option<String> {
        match &self.inspected {
            Inspected::Cell { value, .. } => crate::yank::grid_cell(value),
            Inspected::Row { values, .. } => Some(crate::yank::grid_row(&crate::model::Row {
                values: values.clone(),
            })),
        }
    }

    /// The indent dropped, and in row mode the column name too.
    #[must_use]
    pub fn line(&self) -> Option<String> {
        let (value, line) = self.at_cursor()?;

        match value {
            Value::Null => None,
            _ => Some(line.trim_start().to_owned()),
        }
    }

    fn at_cursor(&self) -> Option<(&Value, String)> {
        let mut at = self.cursor;

        for value in self.inspected.values() {
            let laid = lines(value);

            if at < laid.len() {
                return laid.into_iter().nth(at).map(|line| (value, line));
            }
            at -= laid.len();
        }

        None
    }

    #[must_use]
    pub fn title(&self) -> Vec<Segment> {
        let mut segments = match &self.subject {
            Subject::Table(table) => table.segments(),
            Subject::Typed(_) => vec![Segment::plain(self.subject.label())],
        };

        segments.push(Segment::plain(match &self.inspected {
            Inspected::Cell { column, .. } => format!(" · {column}"),
            Inspected::Row { .. } if self.is_inserting() => " · new row".to_owned(),
            Inspected::Row { number, .. } => format!(" · row {number}"),
        }));

        segments
    }

    /// `None` over a row and over a null.
    #[must_use]
    pub fn size(&self) -> Option<String> {
        let Inspected::Cell { value, .. } = &self.inspected else {
            return None;
        };

        let bytes = match value {
            Value::Null => return None,
            Value::Blob(bytes) => bytes.len(),
            Value::Text(text) | Value::Uuid(text) => text.len(),
            other => cell::text(other).len(),
        };

        Some(Bytes::new(bytes as u64).to_string())
    }
}

/// Control characters are dropped here, not by the renderer: wrapping reads
/// `\n` and `\t` as whitespace, and would break where the grid draws none.
#[must_use]
pub fn lines(value: &Value) -> Vec<String> {
    let laid = match value {
        Value::Text(text) | Value::Json(text) => json(text).unwrap_or_else(|| vec![text.clone()]),
        Value::Array(text) => crate::array::parse(text)
            .map_or_else(|| vec![text.clone()], |elements| array_lines(&elements)),
        other => vec![cell::text(other)],
    };

    laid.into_iter()
        .map(|line| line.chars().filter(|c| !c.is_control()).collect())
        .collect()
}

/// Braced, as Postgres writes an array; nulls bare and text unquoted, as the
/// grid draws a cell.
fn array_lines(elements: &[crate::array::Element]) -> Vec<String> {
    use crate::array::Element;

    if elements.is_empty() {
        return vec!["{}".to_owned()];
    }

    let mut lines = vec!["{".to_owned()];
    for (at, element) in elements.iter().enumerate() {
        let mut element = match element {
            Element::Null => vec!["null".to_owned()],
            Element::Text(text) => vec![text.clone()],
            Element::Array(nested) => array_lines(nested),
        };
        if at + 1 < elements.len()
            && let Some(last) = element.last_mut()
        {
            last.push(',');
        }
        lines.extend(element.into_iter().map(|line| format!("  {line}")));
    }
    lines.push("}".to_owned());
    lines
}

/// `None` unless it is safely re-indentable JSON. Hand-rolled: a parsed map
/// would reorder keys and reformat numbers.
fn json(text: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = text.trim().chars().collect();
    if !matches!(chars.first(), Some('{' | '[')) {
        return None;
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut open: Vec<char> = Vec::new();
    let mut at = 0;

    while let Some(&character) = chars.get(at) {
        if at > 0 && open.is_empty() {
            return None;
        }

        match character {
            '"' => at = string(&chars, at, &mut line)?,
            '{' | '[' => {
                let close = if character == '{' { '}' } else { ']' };
                let next = significant(&chars, at + 1);

                line.push(character);
                if chars.get(next) == Some(&close) {
                    line.push(close);
                    at = next;
                } else {
                    open.push(close);
                    broken(&mut lines, &mut line, open.len());
                }
            }
            '}' | ']' => {
                if open.pop() != Some(character) {
                    return None;
                }
                broken(&mut lines, &mut line, open.len());
                line.push(character);
            }
            ',' => {
                line.push(character);
                broken(&mut lines, &mut line, open.len());
            }
            ':' => line.push_str(": "),
            space if space.is_whitespace() => {
                let before = line.chars().last().is_some_and(bare);
                let after = chars.get(significant(&chars, at)).is_some_and(|c| bare(*c));

                if before && after {
                    return None;
                }
            }
            other => line.push(other),
        }

        at += 1;
    }

    if !open.is_empty() {
        return None;
    }

    lines.push(line);
    Some(lines)
}

/// `None` for a string that never closes.
fn string(chars: &[char], at: usize, line: &mut String) -> Option<usize> {
    line.push('"');
    let mut escaped = false;

    for (offset, &character) in chars.iter().enumerate().skip(at + 1) {
        line.push(character);

        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(offset);
        }
    }

    None
}

fn significant(chars: &[char], from: usize) -> usize {
    chars
        .iter()
        .skip(from)
        .position(|character| !character.is_whitespace())
        .map_or(chars.len(), |offset| from + offset)
}

/// What two tokens separated only by whitespace would run together as.
const fn bare(character: char) -> bool {
    !matches!(character, '{' | '}' | '[' | ']' | ',' | ':' | ' ')
}

fn broken(lines: &mut Vec<String>, line: &mut String, depth: usize) {
    lines.push(std::mem::take(line));
    *line = "  ".repeat(depth);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contents::TableRef;

    fn text(value: &str) -> Value {
        Value::Text(value.to_owned())
    }

    #[test]
    fn json_is_indented_two_spaces_a_level_one_member_to_a_line() {
        assert_eq!(
            lines(&text(r#"{"a":1,"b":[true,null],"c":{}}"#)),
            [
                "{",
                r#"  "a": 1,"#,
                r#"  "b": ["#,
                "    true,",
                "    null",
                "  ],",
                r#"  "c": {}"#,
                "}",
            ]
        );
    }

    #[test]
    fn json_keeps_its_key_order_and_its_number_spelling() {
        assert_eq!(
            lines(&text(r#"{"z": 1.50e3, "a": -0}"#)),
            ["{", r#"  "z": 1.50e3,"#, r#"  "a": -0"#, "}"]
        );
    }

    #[test]
    fn braces_and_escaped_quotes_inside_a_string_are_text() {
        assert_eq!(
            lines(&text(r#"{"k":"a {b} [c], \"d\": e"}"#)),
            ["{", r#"  "k": "a {b} [c], \"d\": e""#, "}"]
        );
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_before_detection() {
        assert_eq!(lines(&text("  \n[1, 2]\n ")), ["[", "  1,", "  2", "]"]);
    }

    #[test]
    fn text_that_is_not_json_is_drawn_as_it_is() {
        for raw in [
            r#"{"a": 1"#,
            r#"{"a": "unterminated}"#,
            r#"{"a": 1]"#,
            "[1] trailing",
            "[not json]",
            "plain prose",
        ] {
            assert_eq!(lines(&text(raw)), [raw], "{raw:?}");
        }
    }

    #[test]
    fn a_blob_and_a_scalar_are_drawn_as_the_grid_draws_them() {
        assert_eq!(lines(&Value::Blob(vec![0xca, 0xfe])), ["\\xcafe"]);
        assert_eq!(lines(&Value::Int(7)), ["7"]);
        assert_eq!(lines(&Value::Null), ["null"]);
    }

    #[test]
    fn control_characters_are_dropped_as_the_grid_drops_them() {
        assert_eq!(
            lines(&text("sgr[\u{1b}[31mRED\u{1b}[0m] tab[\t] lf[\n]")),
            ["sgr[[31mRED[0m] tab[] lf[]"]
        );
    }

    fn cell(value: Value) -> Inspector {
        Inspector::opened(
            0,
            Subject::Table(TableRef::new("shop", "orders")),
            Inspected::Cell {
                at: 0,
                column: "note".to_owned(),
                value,
            },
        )
    }

    #[test]
    fn a_cell_is_sized_in_the_bytes_it_holds() {
        assert_eq!(cell(text("héllo")).size().as_deref(), Some("6 B"));
        assert_eq!(
            cell(Value::Blob(vec![0; 2048])).size().as_deref(),
            Some("2.0 KB")
        );
        assert_eq!(cell(Value::Null).size(), None);
    }

    #[test]
    fn the_cursor_stops_at_the_last_line_of_what_is_open() {
        let mut cell = cell(text("plain prose"));

        cell.page_down(40);

        assert_eq!(cell.cursor(), 0);
    }

    #[test]
    fn the_title_names_the_subject_and_the_column_or_the_row() {
        assert_eq!(
            crate::style::text(&cell(text("x")).title()),
            "shop.orders · note"
        );

        let row = Inspector::opened(
            0,
            Subject::Typed("SELECT *\n  FROM t".to_owned()),
            Inspected::Row {
                number: 3,
                columns: vec!["id".to_owned()],
                values: vec![Value::Int(1)],
            },
        );
        assert_eq!(crate::style::text(&row.title()), "SELECT * FROM t · row 3");
    }

    #[test]
    fn a_cell_yanks_bare_and_a_row_as_one_csv_line() {
        assert_eq!(cell(text("a,b")).yank().as_deref(), Some("a,b"));
        assert_eq!(cell(Value::Null).yank(), None);

        let row = Inspector::opened(
            0,
            Subject::Table(TableRef::new("shop", "orders")),
            Inspected::Row {
                number: 1,
                columns: vec!["id".to_owned(), "note".to_owned()],
                values: vec![Value::Int(1), text("a,b")],
            },
        );
        assert_eq!(row.yank().as_deref(), Some(r#"1,"a,b""#));
    }

    #[test]
    fn an_array_is_laid_out_an_element_a_line_nested_arrays_indented() {
        assert_eq!(
            lines(&Value::Array(r#"{{1,2},{NULL,"a, b"}}"#.to_owned())),
            [
                "{",
                "  {",
                "    1,",
                "    2",
                "  },",
                "  {",
                "    null,",
                "    a, b",
                "  }",
                "}"
            ]
        );
        assert_eq!(lines(&Value::Array("{}".to_owned())), ["{}"]);
    }

    #[test]
    fn jsonb_as_postgres_prints_it_is_laid_out_without_reordering() {
        assert_eq!(
            lines(&Value::Text(r#"{"b": [1, 2], "a": null}"#.to_owned())),
            [
                "{",
                r#"  "b": ["#,
                "    1,",
                "    2",
                "  ],",
                r#"  "a": null"#,
                "}"
            ]
        );
    }

    #[test]
    fn a_rows_lines_run_on_from_one_field_to_the_next() {
        let mut row = Inspector::opened(
            0,
            Subject::Table(TableRef::new("shop", "orders")),
            Inspected::Row {
                number: 1,
                columns: vec!["id".to_owned(), "tags".to_owned(), "note".to_owned()],
                values: vec![Value::Int(7), Value::Array("{a,b}".to_owned()), Value::Null],
            },
        );

        assert_eq!(row.line_count(), 6, "one, four braced, one");
        assert_eq!(row.line().as_deref(), Some("7"));

        row.down();
        row.down();
        assert_eq!(row.line().as_deref(), Some("a,"), "inside the second field");

        row.to_end();
        assert_eq!(row.line(), None, "the null field");
    }
}
