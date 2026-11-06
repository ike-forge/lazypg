//! Postgres's text form of an array read back into its elements: what the
//! inspector lays out one per line and export writes as a JSON array.

/// Elements stay text: the array's element type is not carried, and a value
/// rendered once is not re-rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Element {
    Null,
    Text(String),
    Array(Vec<Element>),
}

/// `None` for text that is not an array literal. The `[1:2]=` bounds prefix
/// Postgres writes for an array not based at 1 is dropped.
#[must_use]
pub fn parse(text: &str) -> Option<Vec<Element>> {
    let body = match text.split_once("={") {
        Some((bounds, rest)) if bounds.starts_with('[') => format!("{{{rest}"),
        _ => text.to_owned(),
    };
    let chars: Vec<char> = body.chars().collect();
    let (elements, end) = array(&chars, 0)?;

    (end == chars.len()).then_some(elements)
}

/// `at` is on the `{`; returns the elements and the index past the `}`.
fn array(chars: &[char], at: usize) -> Option<(Vec<Element>, usize)> {
    if chars.get(at) != Some(&'{') {
        return None;
    }
    let mut elements = Vec::new();
    let mut at = at + 1;

    if chars.get(at) == Some(&'}') {
        return Some((elements, at + 1));
    }

    loop {
        let (element, next) = match chars.get(at)? {
            '{' => {
                let (nested, next) = array(chars, at)?;
                (Element::Array(nested), next)
            }
            '"' => quoted(chars, at)?,
            _ => bare(chars, at),
        };
        elements.push(element);
        at = next;

        match chars.get(at)? {
            ',' => at += 1,
            '}' => return Some((elements, at + 1)),
            _ => return None,
        }
    }
}

/// A backslash escapes the next character.
fn quoted(chars: &[char], at: usize) -> Option<(Element, usize)> {
    let mut text = String::new();
    let mut at = at + 1;

    loop {
        match chars.get(at)? {
            '\\' => {
                text.push(*chars.get(at + 1)?);
                at += 2;
            }
            '"' => return Some((Element::Text(text), at + 1)),
            other => {
                text.push(*other);
                at += 1;
            }
        }
    }
}

/// Unquoted `NULL` is the null; quoted, it is the text.
fn bare(chars: &[char], at: usize) -> (Element, usize) {
    let end = (at..chars.len())
        .find(|&end| matches!(chars[end], ',' | '}'))
        .unwrap_or(chars.len());
    let text: String = chars[at..end].iter().collect();

    let element = if text.eq_ignore_ascii_case("NULL") {
        Element::Null
    } else {
        Element::Text(text)
    };
    (element, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Element {
        Element::Text(value.to_owned())
    }

    #[test]
    fn a_flat_array_splits_at_its_commas() {
        assert_eq!(
            parse("{1,2,3}"),
            Some(vec![text("1"), text("2"), text("3")])
        );
        assert_eq!(parse("{}"), Some(Vec::new()));
    }

    #[test]
    fn a_quoted_element_keeps_its_commas_braces_and_escaped_quotes() {
        assert_eq!(
            parse(r#"{"a, b","{c}","say \"hi\"",NULL,"NULL"}"#),
            Some(vec![
                text("a, b"),
                text("{c}"),
                text(r#"say "hi""#),
                Element::Null,
                text("NULL"),
            ])
        );
    }

    #[test]
    fn a_nested_array_is_an_element_and_bounds_are_dropped() {
        assert_eq!(
            parse("{{1,2},{3,NULL}}"),
            Some(vec![
                Element::Array(vec![text("1"), text("2")]),
                Element::Array(vec![text("3"), Element::Null]),
            ])
        );
        assert_eq!(parse("[0:1]={7,8}"), Some(vec![text("7"), text("8")]));
    }

    #[test]
    fn text_that_is_not_an_array_literal_is_refused() {
        assert_eq!(parse("plain"), None);
        assert_eq!(parse("{1,2"), None);
        assert_eq!(parse("{1,2}x"), None);
    }
}
