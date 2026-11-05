//! The Structure panel's projection of what the Tables cursor is on. Nothing
//! selected shows nothing: the Tables panel already narrates the connection.

use crate::ddl::DdlTab;
use crate::model::{Catalog, Column, Index, RelationKind, RelationRef, Schema, Table};
use crate::panel::Panel;
use crate::search::{Direction, Search};
use crate::state::State;
use crate::style::Token;
use crate::tree::Node;

use super::PanelBody;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub sections: Vec<Section>,
}

impl Details {
    /// The order the cursor walks. Headings are not rows.
    pub fn rows(&self) -> impl Iterator<Item = &DetailRow> {
        self.sections.iter().flat_map(|section| section.rows.iter())
    }

    #[must_use]
    pub fn row_count(&self) -> usize {
        self.sections.iter().map(|section| section.rows.len()).sum()
    }
}

/// Structural grouping: an empty group disappears without a projection
/// remembering to skip it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// `None` for the identity group, whose rows name themselves.
    pub heading: Option<Heading>,
    pub group: Group,
    /// `None` where the group names no key, so settings never read as key
    /// structure.
    pub token: Option<Token>,
    pub rows: Vec<DetailRow>,
}

/// Separate from [`Section::token`]: a heading is coloured by what it names,
/// its rows by what they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub text: String,
    pub token: Token,
}

impl Heading {
    fn new(text: impl Into<String>, token: Token) -> Self {
        Self {
            text: text.into(),
            token,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Identity,
    Shape,
    Settings,
    /// Apart from [`Group::Shape`], so the blank line between key and indexes
    /// follows from the group change.
    Indexes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailRow {
    KeyColumn {
        name: String,
        data_type: String,
    },
    /// Carries its own kind: nothing above it in the Columns tab says what part
    /// it plays.
    Column {
        name: String,
        data_type: String,
        kind: ColumnKind,
    },
    Property {
        name: String,
        value: String,
    },
    Identity {
        kind: IdentityKind,
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKind {
    Schema,
    Relation(RelationKind),
}

impl IdentityKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Schema => "Schema",
            Self::Relation(kind) => kind.label(),
        }
    }

    #[must_use]
    pub const fn token(self) -> Token {
        match self {
            Self::Schema => Token::Schema,
            Self::Relation(_) => Token::Table,
        }
    }
}

/// A key column that also references is drawn as key: the key is what
/// addresses the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnKind {
    PrimaryKey,
    ForeignKey,
    Regular,
}

impl ColumnKind {
    /// The one thing marked with a glyph; the rest travels as the token.
    #[must_use]
    pub const fn is_key(self) -> bool {
        matches!(self, Self::PrimaryKey)
    }

    #[must_use]
    pub const fn token(self) -> Token {
        match self {
            Self::PrimaryKey => Token::PrimaryKey,
            Self::ForeignKey => Token::ForeignKey,
            Self::Regular => Token::RegularColumn,
        }
    }

    fn of(table: &Table, column: &str) -> Self {
        if table.is_key(column) {
            Self::PrimaryKey
        } else if table.references(column) {
            Self::ForeignKey
        } else {
            Self::Regular
        }
    }
}

pub(super) fn body(state: &State) -> PanelBody {
    match of(state) {
        Some(details) => scrolled(state, details),
        None => PanelBody::Blank,
    }
}

/// A range of several rows also answers `None`.
pub(super) fn of(state: &State) -> Option<Details> {
    if state.tables.selects_many() {
        return None;
    }

    let catalog = state.schema()?;

    let details = match (state.tables.selected(catalog)?, shown_tab(state)) {
        (Node::Table { keyspace, table }, DdlTab::Ddl) => of_table(catalog, keyspace, table),
        (Node::Table { keyspace, table }, DdlTab::Columns) => columns_of(keyspace, table),
        (Node::Table { keyspace, table }, DdlTab::Indexes) => indexes_of(keyspace, table),
        (Node::Schema { keyspace, .. }, _) => of_schema(keyspace),
    };

    Some(details)
}

/// No Columns tab for a schema: an empty tab teaches the wrong thing.
pub(super) fn tabs_for(state: &State) -> &'static [DdlTab] {
    if describes_a_table(state) {
        &DdlTab::ALL
    } else {
        &[DdlTab::Ddl]
    }
}

/// The held tab is not reset, so stepping to the schema and back keeps it.
pub(super) fn shown_tab(state: &State) -> DdlTab {
    let tab = state.ddl.tab();

    if tabs_for(state).contains(&tab) {
        tab
    } else {
        DdlTab::Ddl
    }
}

fn describes_a_table(state: &State) -> bool {
    matches!(
        state
            .schema()
            .and_then(|schema| state.tables.selected(schema)),
        Some(Node::Table { .. })
    )
}

fn scrolled(state: &State, details: Details) -> PanelBody {
    if details.sections.is_empty() {
        return PanelBody::Blank;
    }

    let flash = super::flash_over(state, Panel::Ddl);

    PanelBody::Details {
        details,
        selected: state.ddl.cursor.index(),
        offset: state.ddl.scroll.offset(),
        ranges: flash.map_or_else(
            || state.ddl.selection.ranges(state.ddl.cursor.index()),
            |flash| flash.rows().clone(),
        ),
        flash: flash.is_some(),
    }
}

impl DetailRow {
    /// Every row is drawn regardless; this decides where `n` stops.
    #[must_use]
    pub(crate) fn matches(&self, search: &Search) -> bool {
        match self {
            Self::KeyColumn { name, data_type }
            | Self::Column {
                name, data_type, ..
            } => search.matches(name) || search.matches(data_type),
            Self::Property { name, value } => search.matches(name) || search.matches(value),
            // Labels are the same on every table and would tell `n` nothing.
            Self::Identity { name, .. } => search.matches(name),
        }
    }
}

/// Over `Details::rows`, the sequence the cursor counts.
#[must_use]
pub(crate) fn next_match(
    details: &Details,
    search: &Search,
    from: usize,
    direction: Direction,
) -> Option<usize> {
    let rows: Vec<&DetailRow> = details.rows().collect();
    let len = rows.len();

    (0..len)
        .scan(from, |at, _| {
            *at = direction.step(*at, len);
            Some(*at)
        })
        .find(|at| rows[*at].matches(search))
}

fn property(name: impl Into<String>, value: impl Into<String>) -> DetailRow {
    DetailRow::Property {
        name: name.into(),
        value: value.into(),
    }
}

fn identity(schema: &Schema, table: Option<&Table>) -> Section {
    let row = |kind: IdentityKind, name: &str| DetailRow::Identity {
        kind,
        name: name.to_owned(),
    };

    Section {
        heading: None,
        group: Group::Identity,
        token: None,
        rows: std::iter::once(row(IdentityKind::Schema, &schema.name))
            .chain(table.map(|table| row(IdentityKind::Relation(table.kind), &table.name)))
            .collect(),
    }
}

fn key_column(column: &Column) -> DetailRow {
    DetailRow::KeyColumn {
        name: column.name.clone(),
        data_type: column.data_type.clone(),
    }
}

fn primary_key_section(table: &Table) -> Section {
    Section {
        heading: Some(Heading::new("Primary Key", Token::PrimaryKey)),
        group: Group::Shape,
        token: Some(Token::PrimaryKey),
        rows: table.key_columns().map(key_column).collect(),
    }
}

/// One row per key: its columns, then `schema.table(columns)`. No arrow: a
/// wrap would strand it on a line of its own.
fn foreign_keys_section(table: &Table) -> Section {
    Section {
        heading: Some(Heading::new("Foreign Keys", Token::ForeignKey)),
        group: Group::Shape,
        token: Some(Token::ForeignKey),
        rows: table
            .foreign_keys
            .iter()
            .map(|key| {
                property(
                    key.columns.join(", "),
                    format!(
                        "{}({})",
                        qualified(&key.target),
                        key.target_columns.join(", ")
                    ),
                )
            })
            .collect(),
    }
}

/// What a `DELETE` here has to answer to.
fn referenced_by_section(catalog: &Catalog, schema: &Schema, table: &Table) -> Section {
    let target = RelationRef {
        schema: schema.name.clone(),
        table: table.name.clone(),
    };

    Section {
        heading: Some(Heading::new("Referenced By", Token::ForeignKey)),
        group: Group::Shape,
        token: Some(Token::ForeignKey),
        rows: catalog
            .referencing(&target)
            .map(|(from, key)| {
                property(
                    format!("{}({})", qualified(&from), key.columns.join(", ")),
                    format!("on delete {}", key.on_delete.to_lowercase()),
                )
            })
            .collect(),
    }
}

fn qualified(at: &RelationRef) -> String {
    format!("{}.{}", at.schema, at.table)
}

fn settings(heading: &str, rows: Vec<DetailRow>) -> Section {
    Section {
        heading: Some(Heading::new(heading, Token::Section)),
        group: Group::Settings,
        token: None,
        rows,
    }
}

fn of_table(catalog: &Catalog, schema: &Schema, table: &Table) -> Details {
    let constraints = table
        .constraints
        .iter()
        .map(|constraint| property(constraint.name.clone(), constraint.definition.clone()))
        .collect();
    let partitions = table
        .partitions
        .iter()
        .map(|partition| property(partition.clone(), ""))
        .collect();
    let about = table
        .comment
        .iter()
        .map(|comment| property("comment", comment.clone()))
        .chain(
            table
                .definition
                .iter()
                .map(|definition| property("definition", definition.trim().to_owned())),
        )
        .collect();

    Details {
        sections: populated([
            identity(schema, Some(table)),
            primary_key_section(table),
            foreign_keys_section(table),
            referenced_by_section(catalog, schema, table),
            settings("Constraints", constraints),
            settings("Partitions", partitions),
            settings("About", about),
        ]),
    }
}

/// The primary key leads, drawn as the DDL tab draws it, so the tab reads as
/// one list of ways in. Its backing index is not repeated.
fn indexes_of(schema: &Schema, table: &Table) -> Details {
    let mut sections = populated([identity(schema, Some(table)), primary_key_section(table)]);

    sections.extend(index_sections(table));

    Details { sections }
}

fn index_sections(table: &Table) -> Vec<Section> {
    let indexes: Vec<&Index> = table
        .indexes
        .iter()
        .filter(|index| !index.primary)
        .collect();
    if indexes.is_empty() {
        return vec![no_indexes()];
    }

    indexes
        .into_iter()
        .map(|index| Section {
            heading: Some(Heading::new(index.name.clone(), Token::Section)),
            group: Group::Indexes,
            token: None,
            rows: index_rows(table, index),
        })
        .collect()
}

/// An expression key has no column, so no type.
fn index_rows(table: &Table, index: &Index) -> Vec<DetailRow> {
    let keys = index.keys.iter().map(|key| DetailRow::KeyColumn {
        name: key.clone(),
        data_type: table
            .column(key)
            .map(|column| column.data_type.clone())
            .unwrap_or_default(),
    });
    let traits = [
        Some(property("using", index.method.clone())),
        index.unique.then(|| property("unique", "")),
        index
            .predicate
            .as_ref()
            .map(|predicate| property("where", predicate.clone())),
    ];

    keys.chain(traits.into_iter().flatten()).collect()
}

/// A row, not a bare heading: a heading with nothing under it reads as a failed
/// load.
fn no_indexes() -> Section {
    Section {
        heading: Some(Heading::new("Indexes", Token::Section)),
        group: Group::Indexes,
        token: None,
        rows: vec![property("none", "")],
    }
}

/// Flat and in column order, as `SELECT *` returns them; the kind travels
/// with each row.
fn columns_of(schema: &Schema, table: &Table) -> Details {
    let rows = table
        .columns()
        .map(|column| DetailRow::Column {
            name: column.name.clone(),
            data_type: column_type(column),
            kind: ColumnKind::of(table, &column.name),
        })
        .collect();

    Details {
        sections: populated([
            identity(schema, Some(table)),
            Section {
                heading: Some(Heading::new(
                    format!("{} columns", table.columns.len()),
                    Token::Section,
                )),
                group: Group::Shape,
                token: None,
                rows,
            },
        ]),
    }
}

/// `not null` beside the type: whether a cell can be empty is part of its
/// type to anyone writing a query.
fn column_type(column: &Column) -> String {
    if column.nullable {
        column.data_type.clone()
    } else {
        format!("{} not null", column.data_type)
    }
}

fn of_schema(schema: &Schema) -> Details {
    let count = |kind: RelationKind| {
        schema
            .tables
            .values()
            .filter(|table| table.kind == kind)
            .count()
    };
    let contents = [
        (
            "tables",
            count(RelationKind::Table) + count(RelationKind::Partitioned),
        ),
        ("views", count(RelationKind::View)),
        ("materialized views", count(RelationKind::MaterializedView)),
        ("foreign tables", count(RelationKind::Foreign)),
        ("sequences", schema.sequences.len()),
        ("functions", schema.functions.len()),
    ]
    .into_iter()
    // Tables always, so an empty schema says it is empty.
    .filter(|(name, n)| *n > 0 || *name == "tables")
    .map(|(name, n)| property(name, n.to_string()))
    .collect();

    Details {
        sections: populated([identity(schema, None), settings("Contents", contents)]),
    }
}

fn populated<const N: usize>(sections: [Section; N]) -> Vec<Section> {
    sections
        .into_iter()
        .filter(|section| !section.rows.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::connection::{Connection, ConnectionState, SchemaState};
    use crate::model::{
        Catalog, Column, ConnId, Constraint, ConstraintKind, DbError, ForeignKey, Index,
        PrimaryKey, RelationKind, RelationRef,
    };
    use crate::search::{Search, Searched};
    use crate::state::State;
    use crate::tree::tests_support::schema_of;

    use super::super::tests_support::{connected, panel_body, state_with, tabled};
    use super::*;

    fn ddl_body(state: &State) -> PanelBody {
        panel_body(state, 1)
    }

    fn details(state: &State) -> Details {
        let PanelBody::Details { details, .. } = ddl_body(state) else {
            panic!("a selected row has something to describe");
        };
        details
    }

    fn headings(details: &Details) -> Vec<&str> {
        details
            .sections
            .iter()
            .filter_map(|section| {
                section
                    .heading
                    .as_ref()
                    .map(|heading| heading.text.as_str())
            })
            .collect()
    }

    fn identity_rows(details: &Details) -> Vec<(&str, &str)> {
        details
            .rows()
            .filter_map(|row| match row {
                DetailRow::Identity { kind, name } => Some((kind.label(), name.as_str())),
                _ => None,
            })
            .collect()
    }

    fn section<'a>(details: &'a Details, heading: &str) -> &'a Section {
        details
            .sections
            .iter()
            .find(|section| {
                section
                    .heading
                    .as_ref()
                    .is_some_and(|own| own.text == heading)
            })
            .unwrap_or_else(|| panic!("{heading} is missing"))
    }

    fn not_null(name: &str, data_type: &str) -> Column {
        Column {
            nullable: false,
            ..Column::new(name, data_type)
        }
    }

    fn at(table: &str) -> RelationRef {
        RelationRef {
            schema: "shop".to_owned(),
            table: table.to_owned(),
        }
    }

    /// Key in an order other than column order, one outgoing foreign key.
    fn order_line() -> Table {
        Table {
            name: "order_line".to_owned(),
            columns: vec![
                not_null("order_id", "bigint"),
                not_null("line_no", "integer"),
                not_null("customer_id", "bigint"),
                Column::new("note", "text"),
            ],
            primary_key: Some(PrimaryKey {
                name: "order_line_pkey".to_owned(),
                columns: vec!["order_id".to_owned(), "line_no".to_owned()],
            }),
            foreign_keys: vec![ForeignKey {
                name: "order_line_customer_id_fkey".to_owned(),
                columns: vec!["customer_id".to_owned()],
                target: at("customer"),
                target_columns: vec!["id".to_owned()],
                on_update: "NO ACTION".to_owned(),
                on_delete: "CASCADE".to_owned(),
            }],
            indexes: vec![
                Index {
                    name: "order_line_pkey".to_owned(),
                    definition: String::new(),
                    keys: vec!["order_id".to_owned(), "line_no".to_owned()],
                    unique: true,
                    primary: true,
                    method: "btree".to_owned(),
                    predicate: None,
                },
                Index {
                    name: "order_line_customer_idx".to_owned(),
                    definition: String::new(),
                    keys: vec!["customer_id".to_owned(), "lower(note)".to_owned()],
                    unique: false,
                    primary: false,
                    method: "btree".to_owned(),
                    predicate: Some("(note IS NOT NULL)".to_owned()),
                },
            ],
            constraints: vec![Constraint {
                name: "order_line_line_no_check".to_owned(),
                kind: ConstraintKind::Check,
                definition: "CHECK ((line_no >= 0))".to_owned(),
            }],
            comment: Some("One line per product.".to_owned()),
            ..Table::default()
        }
    }

    fn customer() -> Table {
        Table {
            name: "customer".to_owned(),
            columns: vec![not_null("id", "bigint")],
            primary_key: Some(PrimaryKey {
                name: "customer_pkey".to_owned(),
                columns: vec!["id".to_owned()],
            }),
            ..Table::default()
        }
    }

    fn catalog(tables: Vec<Table>) -> Catalog {
        let mut shop = Schema::empty("shop");
        for table in tables {
            shop.tables.insert(table.name.clone(), table);
        }
        Catalog {
            schemas: [("shop".to_owned(), shop)].into_iter().collect(),
        }
    }

    /// The cursor lands on the first table, alphabetically.
    fn browsing_first(tables: Vec<Table>) -> State {
        let schema = catalog(tables);
        let mut state = connected(schema.clone());
        state.tables.open(&schema);
        state.tables.open(&schema);
        state
    }

    fn browsing(table: Table) -> State {
        browsing_first(vec![table])
    }

    fn on_tab(state: &State, tab: DdlTab) -> State {
        let mut state = state.clone();
        state.ddl.show(tab);
        state
    }

    #[test]
    fn a_table_reads_key_references_then_settings() {
        assert_eq!(
            headings(&details(&browsing(order_line()))),
            ["Primary Key", "Foreign Keys", "Constraints", "About"],
            "nothing references it, so no Referenced By"
        );
    }

    #[test]
    fn the_primary_key_is_listed_in_key_order_with_types() {
        assert_eq!(
            section(&details(&browsing(order_line())), "Primary Key").rows,
            [
                DetailRow::KeyColumn {
                    name: "order_id".to_owned(),
                    data_type: "bigint".to_owned(),
                },
                DetailRow::KeyColumn {
                    name: "line_no".to_owned(),
                    data_type: "integer".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_foreign_key_names_its_columns_and_where_they_point() {
        assert_eq!(
            section(&details(&browsing(order_line())), "Foreign Keys").rows,
            [DetailRow::Property {
                name: "customer_id".to_owned(),
                value: "shop.customer(id)".to_owned(),
            }]
        );
    }

    #[test]
    fn a_referenced_table_lists_who_points_at_it_and_what_a_delete_does() {
        let state = browsing_first(vec![customer(), order_line()]);

        assert_eq!(
            section(&details(&state), "Referenced By").rows,
            [DetailRow::Property {
                name: "shop.order_line(customer_id)".to_owned(),
                value: "on delete cascade".to_owned(),
            }]
        );
    }

    #[test]
    fn a_view_says_what_it_is_and_shows_its_definition() {
        let view = Table {
            name: "totals".to_owned(),
            kind: RelationKind::View,
            columns: vec![Column::new("total", "numeric")],
            definition: Some(" SELECT sum(1) AS total;".to_owned()),
            ..Table::default()
        };
        let details = details(&browsing(view));

        assert_eq!(
            identity_rows(&details),
            [("Schema", "shop"), ("View", "totals")]
        );
        assert_eq!(
            section(&details, "About").rows,
            [DetailRow::Property {
                name: "definition".to_owned(),
                value: "SELECT sum(1) AS total;".to_owned(),
            }]
        );
    }

    #[test]
    fn a_partitioned_table_lists_its_partitions() {
        let parent = Table {
            name: "measurement".to_owned(),
            kind: RelationKind::Partitioned,
            partitions: vec!["measurement_2026".to_owned()],
            ..Table::default()
        };

        assert_eq!(
            section(&details(&browsing(parent)), "Partitions").rows,
            [DetailRow::Property {
                name: "measurement_2026".to_owned(),
                value: String::new(),
            }]
        );
    }

    #[test]
    fn a_keyless_table_draws_no_key_section() {
        let keyless = Table {
            primary_key: None,
            ..order_line()
        };

        assert!(!headings(&details(&browsing(keyless))).contains(&"Primary Key"));
    }

    #[test]
    fn the_columns_tab_lists_every_column_in_table_order_with_its_part() {
        let details = details(&on_tab(&browsing(order_line()), DdlTab::Columns));

        assert_eq!(headings(&details), ["4 columns"]);
        assert_eq!(
            section(&details, "4 columns").rows,
            [
                DetailRow::Column {
                    name: "order_id".to_owned(),
                    data_type: "bigint not null".to_owned(),
                    kind: ColumnKind::PrimaryKey,
                },
                DetailRow::Column {
                    name: "line_no".to_owned(),
                    data_type: "integer not null".to_owned(),
                    kind: ColumnKind::PrimaryKey,
                },
                DetailRow::Column {
                    name: "customer_id".to_owned(),
                    data_type: "bigint not null".to_owned(),
                    kind: ColumnKind::ForeignKey,
                },
                DetailRow::Column {
                    name: "note".to_owned(),
                    data_type: "text".to_owned(),
                    kind: ColumnKind::Regular,
                },
            ]
        );
    }

    #[test]
    fn the_indexes_tab_leads_with_the_key_and_skips_the_index_behind_it() {
        let details = details(&on_tab(&browsing(order_line()), DdlTab::Indexes));

        assert_eq!(
            headings(&details),
            ["Primary Key", "order_line_customer_idx"],
            "the key is the index every keyed table has, so it reads first, once"
        );
    }

    #[test]
    fn an_index_lists_its_keys_its_method_and_its_predicate() {
        let details = details(&on_tab(&browsing(order_line()), DdlTab::Indexes));

        assert_eq!(
            section(&details, "order_line_customer_idx").rows,
            [
                DetailRow::KeyColumn {
                    name: "customer_id".to_owned(),
                    data_type: "bigint".to_owned(),
                },
                DetailRow::KeyColumn {
                    name: "lower(note)".to_owned(),
                    data_type: String::new(),
                },
                DetailRow::Property {
                    name: "using".to_owned(),
                    value: "btree".to_owned(),
                },
                DetailRow::Property {
                    name: "where".to_owned(),
                    value: "(note IS NOT NULL)".to_owned(),
                },
            ],
            "an expression key has no column to take a type from"
        );
    }

    #[test]
    fn a_table_with_no_index_says_so_in_a_row_rather_than_a_bare_heading() {
        let details = details(&on_tab(&browsing(customer()), DdlTab::Indexes));

        assert_eq!(
            section(&details, "Indexes").rows,
            [DetailRow::Property {
                name: "none".to_owned(),
                value: String::new(),
            }]
        );
    }

    #[test]
    fn a_schema_counts_what_it_holds_and_offers_only_its_own_tab() {
        let mut state = tabled(&[("shop", &["orders", "users"])]);
        state.ddl.show(DdlTab::Columns);

        assert_eq!(tabs_for(&state), [DdlTab::Ddl]);
        assert_eq!(shown_tab(&state), DdlTab::Ddl);
        assert_eq!(identity_rows(&details(&state)), [("Schema", "shop")]);
        assert_eq!(
            section(&details(&state), "Contents").rows,
            [DetailRow::Property {
                name: "tables".to_owned(),
                value: "2".to_owned(),
            }]
        );
    }

    #[test]
    fn a_table_is_offered_every_tab_and_keeps_the_one_it_was_left_on() {
        let mut state = browsing(order_line());
        state.ddl.show(DdlTab::Columns);

        assert_eq!(tabs_for(&state), DdlTab::ALL);
        assert_eq!(shown_tab(&state), DdlTab::Columns);
    }

    #[test]
    fn the_identity_rows_open_the_body_and_the_cursor_walks_them() {
        let details = details(&browsing(order_line()));

        assert_eq!(
            identity_rows(&details),
            [("Schema", "shop"), ("Table", "order_line")]
        );
        assert!(matches!(
            details.rows().next(),
            Some(DetailRow::Identity {
                kind: IdentityKind::Schema,
                ..
            })
        ));
    }

    #[test]
    fn the_projection_carries_the_panels_own_offset_and_selection() {
        let mut state = browsing(order_line());
        state.ddl.scroll.to_end(9, 3);
        state.ddl.cursor.go_to(3, 6);

        let PanelBody::Details {
            offset, selected, ..
        } = ddl_body(&state)
        else {
            panic!("a selected table has something to describe");
        };

        assert_eq!((offset, selected), (6, 3));
    }

    fn searched_ddl(term: &str) -> State {
        let mut state = browsing(order_line());
        state.apply_search(Searched::Ddl, Search::new(term));
        state
    }

    #[test]
    fn a_search_drops_no_row_and_empties_no_section() {
        let whole = details(&browsing(order_line()));
        let searched = details(&searched_ddl("customer_id"));

        assert_eq!(searched.rows().count(), whole.rows().count());
        assert!(headings(&searched).contains(&"About"));
    }

    #[test]
    fn the_next_match_moves_forward_wraps_and_none_is_none() {
        let state = searched_ddl("line");
        let search = state.ddl.search().expect("applied above");
        let details = details(&state);

        let first = next_match(&details, search, 0, Direction::Forwards).expect("a match");
        let second = next_match(&details, search, first, Direction::Forwards).expect("another");
        assert!(second > first, "{first} then {second}");

        let lone = searched_ddl("customer.id");
        let none = lone.ddl.search().expect("applied above");
        assert_eq!(next_match(&details, none, 0, Direction::Forwards), None);
    }

    #[test]
    fn a_row_matches_on_its_name_its_type_or_its_value() {
        let term = |text: &str| Search::new(text).expect("non-empty");
        let column = DetailRow::KeyColumn {
            name: "order_id".to_owned(),
            data_type: "bigint".to_owned(),
        };
        let property = DetailRow::Property {
            name: "comment".to_owned(),
            value: "One line per product.".to_owned(),
        };

        assert!(column.matches(&term("order")));
        assert!(column.matches(&term("bigint")));
        assert!(property.matches(&term("per product")));
    }

    #[test]
    fn a_multi_row_selection_leaves_the_panel_describing_nothing() {
        let mut state = tabled(&[("shop", &["orders", "users"])]);
        let schema = state.schema().expect("just loaded").clone();
        state.tables.toggle(&schema);

        state.tables.selection.start(state.tables.cursor().index());
        state.move_tree_cursor(state.tables.cursor().index() + 1);

        assert!(of(&state).is_none());
    }

    #[test]
    fn a_panel_with_nothing_selected_stays_blank_rather_than_repeating_the_tables_panel() {
        for state in [
            State::default(),
            state_with(ConnectionState::Connecting {
                conn: ConnId::next(),
                name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            }),
            state_with(ConnectionState::Failed(DbError::Timeout)),
            state_with(ConnectionState::Connected(Connection {
                id: ConnId::next(),
                name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
                posture: crate::posture::Posture::ReadOnly,
                schema: SchemaState::Loading,
            })),
            connected(schema_of(&[])),
        ] {
            assert_eq!(ddl_body(&state), PanelBody::Blank);
        }
    }
}
