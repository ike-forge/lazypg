//! SQL split into the runs a reader distinguishes. A scanner, not a parser: a
//! formatter that reordered a misparsed clause would change the statement.

use super::keywords;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Keyword,
    Identifier,
    Literal,
    Comment,
    Punctuation,
    Whitespace,
}

/// `text` is verbatim; the formatter only recases keywords.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Piece {
    pub kind: Kind,
    pub text: String,
}

/// Lossless: concatenating every `Piece::text` reproduces the input.
pub(crate) fn pieces(text: &str) -> Vec<Piece> {
    let source: Vec<char> = text.chars().collect();
    let mut pieces = Vec::new();
    let mut at = 0;

    while at < source.len() {
        let (kind, len) = run(&source, at);
        pieces.push(Piece {
            kind,
            text: source[at..at + len].iter().collect(),
        });
        at += len;
    }

    pieces
}

/// Never zero.
fn run(source: &[char], at: usize) -> (Kind, usize) {
    let first = source[at];

    if first.is_whitespace() {
        return (
            Kind::Whitespace,
            while_matching(source, at, char::is_whitespace),
        );
    }

    if let Some(len) = comment(source, at) {
        return (Kind::Comment, len);
    }

    if first == '\'' {
        return (Kind::Literal, quoted(source, at, '\''));
    }

    if first == '"' {
        return (Kind::Identifier, quoted(source, at, '"'));
    }

    // `E'…'` escapes with backslashes; `B'…'`, `X'…'` and `U&'…'` only change
    // what the quoted text means.
    if let Some(len) = prefixed_string(source, at) {
        return (Kind::Literal, len);
    }

    if first.is_ascii_digit()
        || (first == '.' && source.get(at + 1).is_some_and(char::is_ascii_digit))
    {
        return (Kind::Literal, number(source, at));
    }

    if first.is_alphabetic() || first == '_' {
        let len = while_matching(source, at, |c| c.is_alphanumeric() || c == '_' || c == '$');
        let word: String = source[at..at + len].iter().collect();

        if keywords::is_value_word(&word) {
            return (Kind::Literal, len);
        }

        let kind = if keywords::is_keyword(&word) {
            Kind::Keyword
        } else {
            Kind::Identifier
        };

        return (kind, len);
    }

    if first == '$' {
        // `$1`: a parameter stands for a value.
        if source.get(at + 1).is_some_and(char::is_ascii_digit) {
            return (
                Kind::Literal,
                1 + while_matching(source, at + 1, |c| c.is_ascii_digit()),
            );
        }
        if let Some(len) = dollar_quoted(source, at) {
            return (Kind::Literal, len);
        }
    }

    (Kind::Punctuation, 1)
}

/// An unterminated block runs to the end of the buffer. Blocks nest, as
/// Postgres's do.
fn comment(source: &[char], at: usize) -> Option<usize> {
    let pair = (source[at], *source.get(at + 1)?);

    match pair {
        ('-', '-') => Some(while_matching(source, at, |c| c != '\n')),
        ('/', '*') => {
            let mut depth = 0_usize;
            let mut end = at;
            while end < source.len() {
                match (source[end], source.get(end + 1)) {
                    ('/', Some('*')) => {
                        depth += 1;
                        end += 2;
                    }
                    ('*', Some('/')) => {
                        depth -= 1;
                        end += 2;
                        if depth == 0 {
                            return Some(end - at);
                        }
                    }
                    _ => end += 1,
                }
            }
            Some(source.len() - at)
        }
        _ => None,
    }
}

/// A doubled delimiter is the delimiter. Unterminated runs reach the end of the
/// buffer: that is every buffer mid-typing.
fn quoted(source: &[char], at: usize, quote: char) -> usize {
    let mut end = at + 1;

    while end < source.len() {
        if source[end] == quote {
            if source.get(end + 1) == Some(&quote) {
                end += 2;
                continue;
            }
            return end + 1 - at;
        }
        end += 1;
    }

    source.len() - at
}

/// `E'it\'s'`: a backslash escapes the next character, a doubled quote still
/// is a quote.
fn escaped(source: &[char], at: usize) -> usize {
    let mut end = at + 1;

    while end < source.len() {
        match source[end] {
            '\\' => end += 2,
            '\'' if source.get(end + 1) == Some(&'\'') => end += 2,
            '\'' => return end + 1 - at,
            _ => end += 1,
        }
    }

    source.len() - at
}

fn prefixed_string(source: &[char], at: usize) -> Option<usize> {
    let first = source[at].to_ascii_uppercase();
    match (first, source.get(at + 1), source.get(at + 2)) {
        ('E', Some('\''), _) => Some(1 + escaped(source, at + 1)),
        ('B' | 'X' | 'N', Some('\''), _) => Some(1 + quoted(source, at + 1, '\'')),
        ('U', Some('&'), Some('\'')) => Some(2 + quoted(source, at + 2, '\'')),
        _ => None,
    }
}

/// `$tag$ … $tag$`, the tag empty or an identifier. `None` when the opening
/// is not one, so a lone `$` is punctuation.
fn dollar_quoted(source: &[char], at: usize) -> Option<usize> {
    let tag_len = while_matching(source, at + 1, |c| c.is_alphanumeric() || c == '_');
    if source.get(at + 1 + tag_len) != Some(&'$')
        || source.get(at + 1).is_some_and(char::is_ascii_digit)
    {
        return None;
    }
    let delimiter = &source[at..at + tag_len + 2];
    let body = at + delimiter.len();

    let close = (body..source.len()).find(|&end| source[end..].starts_with(delimiter));
    Some(close.map_or(source.len() - at, |end| end + delimiter.len() - at))
}

/// `0x`, `0o` and `0b` integers and `_` separators are Postgres 16's; an older
/// server rejects them, which is its error to report.
fn number(source: &[char], at: usize) -> usize {
    if source[at] == '0' && matches!(source.get(at + 1), Some('x' | 'X' | 'o' | 'O' | 'b' | 'B')) {
        return while_matching(source, at + 2, |c| c.is_alphanumeric() || c == '_') + 2;
    }

    let mut end = at;
    while end < source.len() {
        let character = source[end];
        let exponent_sign =
            matches!(character, '+' | '-') && end > at && matches!(source[end - 1], 'e' | 'E');

        let exponent = matches!(character, 'e' | 'E')
            && source
                .get(end + 1)
                .is_some_and(|next| next.is_ascii_digit() || matches!(next, '+' | '-'));

        if !(character.is_ascii_digit()
            || character == '.'
            || character == '_'
            || exponent_sign
            || exponent)
        {
            break;
        }

        end += 1;
    }

    end - at
}

fn while_matching(source: &[char], at: usize, matches: impl Fn(char) -> bool) -> usize {
    let mut end = at;
    while end < source.len() && matches(source[end]) {
        end += 1;
    }

    end - at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, String)> {
        pieces(text)
            .into_iter()
            .filter(|piece| piece.kind != Kind::Whitespace)
            .map(|piece| (piece.kind, piece.text))
            .collect()
    }

    fn kinds_only(text: &str) -> Vec<Kind> {
        kinds(text).into_iter().map(|(kind, _)| kind).collect()
    }

    #[test]
    fn every_character_lands_in_exactly_one_piece() {
        let text = "SELECT * FROM shop.orders WHERE id = 7; -- done\n";

        let rebuilt: String = pieces(text)
            .iter()
            .map(|piece| piece.text.as_str())
            .collect();

        assert_eq!(rebuilt, text);
    }

    #[test]
    fn a_select_splits_into_grammar_names_and_punctuation() {
        assert_eq!(
            kinds_only("SELECT * FROM shop.orders"),
            vec![
                Kind::Keyword,
                Kind::Punctuation,
                Kind::Keyword,
                Kind::Identifier,
                Kind::Punctuation,
                Kind::Identifier,
            ]
        );
    }

    #[test]
    fn a_keyword_is_grammar_in_any_case() {
        assert_eq!(kinds_only("select"), vec![Kind::Keyword]);
    }

    #[test]
    fn a_string_survives_a_doubled_quote_inside_it() {
        assert_eq!(
            kinds("'it''s'"),
            vec![(Kind::Literal, "'it''s'".to_owned())]
        );
    }

    #[test]
    fn a_quoted_name_is_a_name_and_not_a_string() {
        assert_eq!(
            kinds("\"MyKs\".\"Orders\""),
            vec![
                (Kind::Identifier, "\"MyKs\"".to_owned()),
                (Kind::Punctuation, ".".to_owned()),
                (Kind::Identifier, "\"Orders\"".to_owned()),
            ]
        );
    }

    #[test]
    fn an_unterminated_string_runs_to_the_end_rather_than_resynchronising() {
        // Every buffer between the quotes is in this state while typing.
        assert_eq!(kinds("WHERE a = 'ab"), {
            vec![
                (Kind::Keyword, "WHERE".to_owned()),
                (Kind::Identifier, "a".to_owned()),
                (Kind::Punctuation, "=".to_owned()),
                (Kind::Literal, "'ab".to_owned()),
            ]
        });
    }

    #[test]
    fn a_line_comment_ends_at_its_line_and_a_block_comment_at_its_terminator() {
        assert_eq!(
            kinds("-- one\nSELECT /* two */ 1"),
            vec![
                (Kind::Comment, "-- one".to_owned()),
                (Kind::Keyword, "SELECT".to_owned()),
                (Kind::Comment, "/* two */".to_owned()),
                (Kind::Literal, "1".to_owned()),
            ]
        );
    }

    #[test]
    fn a_block_comment_spans_lines() {
        assert_eq!(
            kinds("/* one\ntwo */ SELECT"),
            vec![
                (Kind::Comment, "/* one\ntwo */".to_owned()),
                (Kind::Keyword, "SELECT".to_owned()),
            ]
        );
    }

    /// Version 1 puts the timestamp's high nibbles first, so `now()` uuids
    /// often open with a letter.
    #[test]
    fn a_name_spelled_in_hex_letters_is_still_a_name() {
        assert_eq!(kinds_only("deadbeef"), vec![Kind::Identifier]);
    }

    #[test]
    fn a_subtraction_of_two_numbers_stays_three_pieces() {
        assert_eq!(
            kinds_only("1-2"),
            vec![Kind::Literal, Kind::Punctuation, Kind::Literal]
        );
    }

    #[test]
    fn a_number_takes_its_fraction_and_its_exponent() {
        assert_eq!(kinds("1.5e-3"), vec![(Kind::Literal, "1.5e-3".to_owned())]);
    }

    #[test]
    fn a_blob_is_one_literal() {
        assert_eq!(
            kinds("0xCAFEBABE"),
            vec![(Kind::Literal, "0xCAFEBABE".to_owned())]
        );
    }

    #[test]
    fn the_value_words_are_literals_rather_than_grammar() {
        assert_eq!(
            kinds_only("null true false"),
            vec![Kind::Literal, Kind::Literal, Kind::Literal]
        );
    }

    #[test]
    fn a_multibyte_name_is_one_piece() {
        assert_eq!(kinds("café"), vec![(Kind::Identifier, "café".to_owned())]);
    }

    #[test]
    fn an_escape_string_ends_at_the_first_quote_no_backslash_escapes() {
        assert_eq!(
            kinds(r"E'it\'s' x"),
            [
                (Kind::Literal, r"E'it\'s'".to_owned()),
                (Kind::Identifier, "x".to_owned()),
            ]
        );
    }

    #[test]
    fn a_dollar_quoted_body_is_one_literal_whatever_it_holds() {
        let body = "$fn$ BEGIN RETURN 'x'; END $$ inner $$ $fn$";
        assert_eq!(
            kinds(&format!("{body};")),
            [
                (Kind::Literal, body.to_owned()),
                (Kind::Punctuation, ";".to_owned()),
            ]
        );
        assert_eq!(kinds_only("$$a;b$$"), [Kind::Literal]);
    }

    #[test]
    fn a_parameter_is_a_value_and_a_lone_dollar_is_punctuation() {
        assert_eq!(kinds_only("$1"), [Kind::Literal]);
        assert_eq!(kinds_only("$"), [Kind::Punctuation]);
    }

    #[test]
    fn a_cast_is_punctuation_then_a_type() {
        assert_eq!(
            kinds("1::text"),
            [
                (Kind::Literal, "1".to_owned()),
                (Kind::Punctuation, ":".to_owned()),
                (Kind::Punctuation, ":".to_owned()),
                (Kind::Keyword, "text".to_owned()),
            ]
        );
    }

    #[test]
    fn block_comments_nest_and_a_double_slash_is_no_comment() {
        assert_eq!(
            kinds_only("/* a /* b */ c */x"),
            [Kind::Comment, Kind::Identifier]
        );
        assert_eq!(
            kinds_only("a//b"),
            [
                Kind::Identifier,
                Kind::Punctuation,
                Kind::Punctuation,
                Kind::Identifier,
            ]
        );
    }

    #[test]
    fn a_common_column_name_is_a_name_not_grammar() {
        for word in ["type", "key", "comment", "name", "first"] {
            assert_eq!(kinds_only(word), [Kind::Identifier], "{word}");
        }
    }
}
