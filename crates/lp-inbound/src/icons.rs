//! Nerd Font glyphs drawn in front of names, unconditionally: a terminal never
//! reports its font, and a missing glyph still advances one column, so nothing
//! can probe for it. Here rather than in the view model: a codepoint is not a
//! name.

use lp_domain::model::RelationKind;
use lp_domain::style::Token;
use lp_domain::view::{Group, RowKind};

pub(crate) const DATABASE: &str = "\u{f1c0} ";
/// The solid cut reads larger at one cell.
pub(crate) const SCHEMA: &str = "\u{f084} ";
/// The outline cut: a filled table reads as noise at one cell.
pub(crate) const TABLE: &str = "\u{f0ce} ";
/// A grid of grids: rows live in its partitions.
pub(crate) const PARTITIONED: &str = "\u{f00a} ";
/// What a view is: a way of looking.
pub(crate) const VIEW: &str = "\u{f06e} ";
/// Solid: unlike a view, it holds its rows.
pub(crate) const MATERIALIZED_VIEW: &str = "\u{f1b2} ";
/// The rows are on another server.
pub(crate) const FOREIGN: &str = "\u{f0c1} ";
/// Not the namespace's key: one glyph for two things would equate them.
pub(crate) const KEY: &str = "\u{f030b} ";
/// The outline cut: a filled cog is a blob at one cell.
pub(crate) const SETTINGS: &str = "\u{f013} ";
/// Not the key: an index is not part of the key, and the Indexes tab draws
/// both.
pub(crate) const INDEX: &str = "\u{f002} ";

pub(crate) const fn tree_row(kind: RowKind) -> &'static str {
    match kind {
        RowKind::Database { .. } => DATABASE,
        RowKind::Schema { .. } => SCHEMA,
        RowKind::Table { relation, .. } => match relation {
            RelationKind::Table => TABLE,
            RelationKind::Partitioned => PARTITIONED,
            RelationKind::View => VIEW,
            RelationKind::MaterializedView => MATERIALIZED_VIEW,
            RelationKind::Foreign => FOREIGN,
        },
    }
}

/// The icon keeps its kind's colour whatever the row wears.
pub(crate) const fn token(kind: RowKind) -> Token {
    match kind {
        RowKind::Database { .. } => Token::Database,
        RowKind::Schema { .. } => Token::Schema,
        RowKind::Table { .. } => Token::Table,
    }
}

pub(crate) fn identity(token: Token) -> &'static str {
    match token {
        Token::Database => DATABASE,
        Token::Schema => SCHEMA,
        Token::Table => TABLE,
        _ => "",
    }
}

/// Decided by group: `Token::Section` also heads the Columns tab, which marks
/// its rows instead.
pub(crate) fn section(token: Token, group: Group) -> &'static str {
    match (token, group) {
        (Token::PrimaryKey | Token::ForeignKey, _) => KEY,
        (_, Group::Indexes) => INDEX,
        (_, Group::Settings) => SETTINGS,
        _ => "",
    }
}
