use std::cmp::Ordering;

/// Only the shapes the grid renders or sorts differently; every other type
/// arrives as [`Value::Text`] in Postgres's own rendering.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Boolean(bool),
    Int(i64),
    Float(f64),
    /// `numeric`, as Postgres printed it: arbitrary precision survives.
    Numeric(String),
    /// Any array, as Postgres printed it (`{1,2}`, `{{a,"b c"},{NULL,d}}`).
    /// Kept as text so the grid draws Postgres's own form; the inspector and
    /// export read its structure.
    Array(String),
    /// `json` or `jsonb`, as Postgres printed it: export embeds it rather than
    /// quoting it.
    Json(String),
    Text(String),
    Blob(Vec<u8>),
    Uuid(String),
}

impl Value {
    /// What the grid's sort compares: numbers as numbers, the rest as text,
    /// nulls last whichever way the column is sorted.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "past 2^53 integers tie rather than misorder, as numeric does"
    )]
    pub fn sort_key(&self) -> SortKey {
        match self {
            Self::Null => SortKey::Null,
            Self::Int(value) => SortKey::Number(*value as f64),
            Self::Float(value) => SortKey::Number(*value),
            Self::Numeric(text) => text
                .parse()
                .map_or_else(|_| SortKey::Text(text.clone()), SortKey::Number),
            other => SortKey::Text(crate::cell::text(other)),
        }
    }
}

/// `f64` for every number: `numeric` beyond its precision ties rather than
/// misorders, which a local sort of one page can live with.
#[derive(Debug, Clone, PartialEq)]
pub enum SortKey {
    Number(f64),
    Text(String),
    Null,
}

impl SortKey {
    /// Numbers before text, as a mixed column reads; NaN after every other
    /// number, as Postgres sorts it.
    #[must_use]
    pub fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Number(left), Self::Number(right)) => left.total_cmp(right),
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) | (Self::Text(_), Self::Number(_)) => Ordering::Greater,
            (_, Self::Null) | (Self::Number(_), Self::Text(_)) => Ordering::Less,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(values: Vec<Value>) -> Vec<Value> {
        let mut values = values;
        values.sort_by(|left, right| left.sort_key().compare(&right.sort_key()));
        values
    }

    #[test]
    fn numbers_sort_by_value_not_by_their_text() {
        assert_eq!(
            sorted(vec![
                Value::Int(10),
                Value::Int(9),
                Value::Numeric("9.5".to_owned())
            ]),
            [
                Value::Int(9),
                Value::Numeric("9.5".to_owned()),
                Value::Int(10)
            ]
        );
    }

    #[test]
    fn nan_follows_every_number_and_null_follows_everything() {
        let sorted = sorted(vec![
            Value::Null,
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
            Value::Float(-1.0),
        ]);

        assert!(matches!(sorted[0], Value::Float(f) if f.is_sign_negative() && f.is_finite()));
        assert!(matches!(sorted[1], Value::Float(f) if f.is_infinite()));
        assert!(matches!(sorted[2], Value::Float(f) if f.is_nan()));
        assert_eq!(sorted[3], Value::Null);
    }
}
