//! One [`Value`] as the text a grid draws. In the domain, so every renderer
//! agrees and it tests without a cluster.

use crate::model::Value;
use unicode_normalization::UnicodeNormalization;
use unicode_width::UnicodeWidthChar;

const HEX: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Public for the adapter: a value nested in a tuple or UDT renders as it would
/// alone.
#[must_use]
pub fn text(value: &Value) -> String {
    match value {
        // Told apart from the text "null" by the faint token, not the text.
        Value::Null => "null".to_owned(),
        Value::Boolean(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => float(*value),
        Value::Text(value)
        | Value::Uuid(value)
        | Value::Numeric(value)
        | Value::Array(value)
        | Value::Json(value) => value.clone(),
        Value::Blob(bytes) => hex(bytes),
    }
}

/// [`text`] made safe to draw. A raw ESC, `\n` or `\t` reaches the terminal as
/// a command or a line break and shifts every cell after it; yank and export
/// keep [`text`].
///
/// Nothing zero-width is drawn either. The `ConPTY` that Alacritty ships on
/// Windows (`OpenConsole` 1.24) repaints a line by writing it whole and letting
/// the terminal wrap, and it measures any combining mark, ZWSP or ZWJ sequence
/// differently from the terminal, so the wrap lands off and every line below
/// shifts. NFC first, so a decomposed accent survives as one character.
#[must_use]
pub fn display(value: &Value) -> String {
    text(value)
        .nfc()
        .map(visible)
        .filter(|character| character.width() != Some(0))
        .collect()
}

/// C0 controls take their Unicode control picture, which is one column wide.
/// The rest (C1, bidi overrides and isolates) reorder or move the cursor with
/// nothing to show for it.
fn visible(character: char) -> char {
    match character {
        '\0'..='\u{1f}' => char::from_u32(0x2400 + u32::from(character)).unwrap_or('\u{fffd}'),
        '\u{7f}' => '\u{2421}',
        '\u{80}'..='\u{9f}'
        | '\u{61c}'
        | '\u{200e}'
        | '\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}' => '\u{fffd}',
        other => other,
    }
}

/// Postgres's spellings of the values Rust spells `NaN`, `inf` and `-inf`.
fn float(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned()
    } else {
        value.to_string()
    }
}

/// `bytea_output = hex`, the form Postgres prints and accepts back.
fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(2 + bytes.len() * 2);
    out.push_str("\\x");

    for byte in bytes {
        out.push(HEX[usize::from(byte >> 4)]);
        out.push(HEX[usize::from(byte & 0x0f)]);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_render_the_way_psql_prints_them() {
        assert_eq!(text(&Value::Null), "null");
        assert_eq!(text(&Value::Boolean(true)), "true");
        assert_eq!(text(&Value::Int(-42)), "-42");
        assert_eq!(text(&Value::Float(1.5)), "1.5");
        assert_eq!(text(&Value::Text("ada".to_owned())), "ada");
        assert_eq!(text(&Value::Uuid("8f14e45f".to_owned())), "8f14e45f");
    }

    #[test]
    fn display_replaces_what_would_move_the_terminal() {
        let hostile = Value::Text("a\x1b[2Jb\nc\td\u{7f}e\u{202e}f\u{85}".to_owned());

        assert_eq!(
            display(&hostile),
            "a\u{241b}[2Jb\u{240a}c\u{2409}d\u{2421}e\u{fffd}f\u{fffd}"
        );
        assert_eq!(
            text(&hostile).chars().count(),
            display(&hostile).chars().count()
        );
    }

    #[test]
    fn display_splits_sequences_a_terminal_may_draw_part_by_part() {
        let sequences =
            Value::Text("👩\u{200d}👩\u{200d}👧 🏴\u{e0067}\u{e0062}\u{e007f}".to_owned());

        assert_eq!(display(&sequences), "👩👩👧 🏴");
    }

    #[test]
    fn display_composes_a_decomposed_accent() {
        let decomposed = Value::Text("e\u{301}".to_owned());

        assert_eq!(display(&decomposed), "\u{e9}");
    }

    #[test]
    fn display_drops_marks_with_no_composed_form() {
        let stacked = Value::Text("x\u{301}\u{302} \u{e01}\u{e31}".to_owned());

        assert_eq!(display(&stacked), "x \u{e01}");
    }

    #[test]
    fn display_drops_zero_width_spaces() {
        let spaced = Value::Text("a\u{200b}b\u{feff}c".to_owned());

        assert_eq!(display(&spaced), "abc");
    }

    #[test]
    fn display_leaves_ordinary_text_alone() {
        let text_value = Value::Text("全角 \u{e9} 👩 \u{5d1}".to_owned());

        assert_eq!(display(&text_value), text(&text_value));
    }

    #[test]
    fn a_bytea_is_the_hex_form_postgres_prints_and_accepts_back() {
        assert_eq!(text(&Value::Blob(vec![0x1f, 0x8b, 0x00])), "\\x1f8b00");
        assert_eq!(text(&Value::Blob(Vec::new())), "\\x");
    }

    #[test]
    fn a_float_postgres_cannot_spell_as_rust_does_takes_its_own_spelling() {
        assert_eq!(text(&Value::Float(f64::NAN)), "NaN");
        assert_eq!(text(&Value::Float(f64::INFINITY)), "Infinity");
        assert_eq!(text(&Value::Float(f64::NEG_INFINITY)), "-Infinity");
        assert_eq!(text(&Value::Float(0.5)), "0.5");
    }

    #[test]
    fn a_numeric_keeps_every_digit_postgres_printed() {
        let digits = "3.14159265358979323846264338327950288";
        assert_eq!(text(&Value::Numeric(digits.to_owned())), digits);
    }
}
