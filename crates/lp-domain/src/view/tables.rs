use crate::connection::{ConnectionState, SchemaState};
use crate::feedback::Flash;
use crate::model::{Catalog, RelationKind, SchemaKind};
use crate::panel::Panel;
use crate::state::State;
use crate::style::{Segment, Token};
use crate::tree::{Node, TablesPanel};

use super::{PanelBody, PanelTitle, waiting};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub label: String,
    /// Not derivable from `token`: a system namespace and its tables are both
    /// faint.
    pub kind: RowKind,
    pub token: Token,
}

/// Each row carries the limbs that pick its glyphs: its own, and for a table
/// under a database, its schema's, which decides whether a guide runs past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Database {
        current: bool,
    },
    /// `None` at the root, where a catalog read alone puts it.
    Schema {
        limb: Option<Limb>,
    },
    Table {
        limb: Limb,
        outer: Option<Limb>,
        relation: RelationKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limb {
    Middle,
    Last,
}

impl Limb {
    const fn of(last: bool) -> Self {
        if last { Self::Last } else { Self::Middle }
    }
}

impl RowKind {
    #[must_use]
    pub const fn is_table(self) -> bool {
        matches!(self, Self::Table { .. })
    }
}

/// The connection in the picker's spelling and colours, so the posture reads
/// the same everywhere. Too narrow for the badge, it drops whole: half a
/// posture can read as another.
pub(super) fn title(state: &State, number: char) -> PanelTitle {
    const BASE: &str = "Objects";
    let short = vec![Segment::plain(BASE)];

    let ConnectionState::Connected(connection) = &state.connection else {
        return PanelTitle::abbreviated(number, short.clone(), short);
    };
    let posture = connection.posture;

    PanelTitle::abbreviated(
        number,
        vec![
            Segment::plain(format!("{BASE} ({}, ", connection.name.as_str())),
            Segment::styled(posture.label(), Token::Posture(posture)),
            Segment::plain(")"),
        ],
        short,
    )
}

pub(super) fn body(state: &State) -> PanelBody {
    match &state.connection {
        // The modal owns the failure, the trail the detail.
        ConnectionState::Disconnected | ConnectionState::Failed(_) => waiting("Not connected"),
        // The picker is up or one `ctrl-o` away, so say what to do.
        ConnectionState::Unchosen => waiting("No connection chosen"),
        ConnectionState::Connecting { .. } => waiting("Connecting…"),
        ConnectionState::Connected(connection) => match &connection.schema {
            SchemaState::Loading => waiting("Loading schema…"),
            // The read is over; "Loading schema…" would be false.
            SchemaState::Failed(_) => waiting("No schema"),
            SchemaState::Loaded(schema) => tree(
                &state.tables,
                schema,
                super::flash_over(state, Panel::Objects),
            ),
        },
    }
}

fn tree(tables: &TablesPanel, schema: &Catalog, flash: Option<&Flash>) -> PanelBody {
    let nodes = tables.rows(schema);
    let nested = schema.server.is_some();
    let rows: Vec<TreeRow> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| tree_row(node, limbs(&nodes, index, nested)))
        .collect();

    if rows.is_empty() {
        // A search that kept nothing is not an empty cluster.
        return waiting(if tables.search().is_some() {
            "No match"
        } else {
            "No schemas"
        });
    }

    PanelBody::Tree {
        rows,
        selected: tables.cursor().index(),
        offset: tables.scroll.offset(),
        ranges: flash.map_or_else(
            || tables.selection.ranges(tables.cursor().index()),
            |flash| flash.rows().clone(),
        ),
        flash: flash.is_some(),
    }
}

/// What a row's limbs are, read off its neighbours: siblings are contiguous,
/// so a row is last when the next sibling-or-shallower row is not a sibling.
#[derive(Debug, Clone, Copy)]
struct Limbs {
    own: Limb,
    /// Under a database, everything below the root is drawn one level in.
    nested: bool,
    /// The schema's limb, for a table under a database.
    outer: Option<Limb>,
}

fn depth(node: &Node<'_>) -> usize {
    match node {
        Node::Database { .. } => 0,
        Node::Schema { .. } => 1,
        Node::Table { .. } => 2,
    }
}

fn is_last(nodes: &[Node<'_>], index: usize) -> bool {
    let own = depth(&nodes[index]);
    nodes[index + 1..]
        .iter()
        .map(depth)
        .find(|depth| *depth <= own)
        .is_none_or(|depth| depth < own)
}

fn limbs(nodes: &[Node<'_>], index: usize, nested: bool) -> Limbs {
    let outer = nested
        .then(|| {
            nodes[..index]
                .iter()
                .rposition(|node| matches!(node, Node::Schema { .. }))
        })
        .flatten()
        .map(|schema| Limb::of(is_last(nodes, schema)));

    Limbs {
        own: Limb::of(is_last(nodes, index)),
        nested,
        outer,
    }
}

fn tree_row(node: &Node<'_>, limbs: Limbs) -> TreeRow {
    let (namespace, label, kind) = match node {
        Node::Database { name, current, .. } => {
            return TreeRow {
                label: (*name).to_owned(),
                kind: RowKind::Database { current: *current },
                // Another database is a place to go, not one on screen.
                token: if *current {
                    Token::Database
                } else {
                    Token::Muted
                },
            };
        }
        Node::Schema { namespace, .. } => (
            *namespace,
            namespace.name.clone(),
            RowKind::Schema {
                limb: limbs.nested.then_some(limbs.own),
            },
        ),
        Node::Table { namespace, table } => (
            *namespace,
            table.name.clone(),
            RowKind::Table {
                limb: limbs.own,
                outer: limbs.outer,
                relation: table.kind,
            },
        ),
    };

    let token = match (namespace.kind(), kind) {
        (SchemaKind::System, _) => Token::Faint,
        (SchemaKind::User, RowKind::Table { .. }) => Token::Table,
        (SchemaKind::User, _) => Token::Schema,
    };

    TreeRow { label, kind, token }
}

#[cfg(test)]
mod tests {
    use crate::connection::{Connection, ConnectionState, SchemaState};
    use crate::model::{ConnId, DbError};
    use crate::posture::Posture;
    use crate::selection::Ranges;
    use crate::state::State;
    use crate::style::Token;

    use crate::search::{Search, Searched};

    use super::super::tests_support::{loaded, panel_body, state_with, tabled};
    use super::*;

    fn tables_body(state: &State) -> PanelBody {
        panel_body(state, 0)
    }

    fn tables_title(state: &State) -> PanelTitle {
        super::super::view_model(state).panels[0].title.clone()
    }

    fn open(name: &str, posture: Posture) -> State {
        state_with(ConnectionState::Connected(Connection {
            id: ConnId::next(),
            name: crate::config::ConnectionName::try_new(name).expect("a valid name"),
            posture,
            schema: SchemaState::Loading,
        }))
    }

    #[test]
    fn the_border_names_the_connection_and_its_posture() {
        let title = tables_title(&open("local", Posture::ReadOnly));

        assert_eq!(title.text(), "[1]-Objects (local, read-only)");
    }

    #[test]
    fn the_posture_is_spelled_the_way_the_picker_and_the_config_spell_it() {
        for posture in [Posture::ReadOnly, Posture::Writable, Posture::Protected] {
            let title = tables_title(&open("prod", posture));

            assert_eq!(
                title.text(),
                format!("[1]-Objects (prod, {})", posture.label()),
                "a border that renamed a posture would be teaching a key that does not parse"
            );
        }
    }

    #[test]
    fn the_posture_carries_its_own_colour_and_the_rest_of_the_name_does_not() {
        let title = tables_title(&open("prod", Posture::Writable));

        assert_eq!(
            title.tabs()[0]
                .segments()
                .iter()
                .map(|segment| segment.token)
                .collect::<Vec<_>>(),
            [None, Some(Token::Posture(Posture::Writable)), None]
        );
    }

    #[test]
    fn a_narrow_border_falls_back_to_the_panels_name_alone() {
        let title = tables_title(&open("local", Posture::ReadOnly));

        assert_eq!(title.tabs()[0].short(), "Objects");
    }

    #[test]
    fn a_panel_with_no_connection_names_no_connection() {
        for state in [
            State::default(),
            state_with(ConnectionState::Unchosen),
            state_with(ConnectionState::Failed(DbError::Timeout)),
            state_with(ConnectionState::Connecting {
                conn: ConnId::next(),
                name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            }),
        ] {
            // Including `Connecting`: the posture is the cluster's answer and
            // has not arrived.
            assert_eq!(tables_title(&state).text(), "[1]-Objects");
        }
    }

    #[test]
    fn the_tables_panel_says_what_it_is_waiting_for() {
        assert_eq!(
            tables_body(&State::default()),
            PanelBody::Notice {
                text: "Not connected".to_owned(),
                token: Some(Token::Muted),
            }
        );
        assert_eq!(
            tables_body(&state_with(ConnectionState::Connecting {
                conn: ConnId::next(),
                name: crate::config::ConnectionName::try_new("local").expect("a valid name")
            })),
            PanelBody::Notice {
                text: "Connecting…".to_owned(),
                token: Some(Token::Muted),
            }
        );
        assert_eq!(
            tables_body(&state_with(ConnectionState::Connected(Connection {
                id: ConnId::next(),
                name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
                posture: Posture::ReadOnly,
                schema: SchemaState::Loading,
            }))),
            PanelBody::Notice {
                text: "Loading schema…".to_owned(),
                token: Some(Token::Muted),
            }
        );
    }

    #[test]
    fn a_failed_connection_does_not_spend_the_panel_on_its_error() {
        let body = tables_body(&state_with(ConnectionState::Failed(DbError::Timeout)));

        let PanelBody::Notice { text, token } = body else {
            panic!("a notice");
        };
        assert!(!text.contains("timed out"), "{text}");
        assert_eq!(token, Some(Token::Muted));
    }

    #[test]
    fn schemas_are_listed_with_the_systems_dimmed_and_last() {
        let state = loaded(&["pg_catalog", "typezoo", "shop"]);

        let PanelBody::Tree { rows, selected, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its schemas");
        };

        assert_eq!(
            rows.iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            ["shop", "typezoo", "pg_catalog"]
        );
        assert_eq!(
            rows[0].token,
            Token::Schema,
            "a user namespace is told apart from a user table"
        );
        assert_eq!(rows[2].token, Token::Faint);
        assert_eq!(selected, 0);
    }

    #[test]
    fn a_namespace_with_no_tables_offers_nothing_to_unfold() {
        let state = loaded(&["shop"]);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its schemas");
        };

        assert_eq!(
            rows[0].kind,
            RowKind::Schema { limb: None },
            "a namespace with nothing under it is still a namespace"
        );
    }

    #[test]
    fn a_namespace_with_tables_is_folded_until_it_is_opened() {
        let mut state = tabled(&[("shop", &["orders", "users"])]);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its schemas");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, RowKind::Schema { limb: None });

        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its schemas");
        };
        assert_eq!(
            rows.iter()
                .map(|row| (row.kind, row.label.as_str()))
                .collect::<Vec<_>>(),
            [
                (RowKind::Schema { limb: None }, "shop"),
                (
                    RowKind::Table {
                        limb: Limb::Middle,
                        outer: None,
                        relation: RelationKind::Table
                    },
                    "orders"
                ),
                (
                    RowKind::Table {
                        limb: Limb::Last,
                        outer: None,
                        relation: RelationKind::Table
                    },
                    "users"
                ),
            ]
        );
    }

    #[test]
    fn a_table_is_faint_exactly_when_its_schema_is() {
        let mut state = tabled(&[("pg_catalog", &["tables"])]);
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its schemas");
        };

        assert_eq!(rows[0].token, Token::Faint);
        assert_eq!(
            rows[1].token,
            Token::Faint,
            "one rule per row, not two rules that can drift"
        );
    }

    #[test]
    fn the_selection_is_projected_from_the_cursor() {
        let mut state = loaded(&["shop", "typezoo"]);
        state.move_tree_cursor(1);

        let PanelBody::Tree { selected, .. } = tables_body(&state) else {
            panic!("a loaded schema lists its schemas");
        };

        assert_eq!(selected, 1);
    }

    #[test]
    fn a_cluster_with_no_namespaces_says_so_rather_than_listing_nothing() {
        assert_eq!(
            tables_body(&loaded(&[])),
            PanelBody::Notice {
                text: "No schemas".to_owned(),
                token: Some(Token::Muted),
            }
        );
    }

    #[test]
    fn a_search_that_matches_nothing_says_so_rather_than_naming_the_cluster_empty() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.apply_search(Searched::Tables, Search::new("zzz"));

        assert_eq!(
            tables_body(&state),
            PanelBody::Notice {
                text: "No match".to_owned(),
                token: Some(Token::Muted),
            }
        );
    }

    #[test]
    fn a_search_keeps_the_namespaces_it_reaches() {
        let mut state = tabled(&[("shop", &["orders"]), ("logs", &["spans"])]);
        state.apply_search(Searched::Tables, Search::new("orders"));

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a search that matches must leave a tree");
        };

        assert_eq!(
            rows.iter()
                .map(|row| row.label.as_str())
                .collect::<Vec<_>>(),
            ["shop", "orders"]
        );
    }

    #[test]
    fn a_failed_schema_read_does_not_spend_the_panel_on_its_error() {
        let state = state_with(ConnectionState::Connected(Connection {
            id: ConnId::next(),
            name: crate::config::ConnectionName::try_new("local").expect("a valid name"),
            posture: Posture::ReadOnly,
            schema: SchemaState::Failed(DbError::Invalid("unauthorized".to_owned())),
        }));

        let PanelBody::Notice { text, token } = tables_body(&state) else {
            panic!("a notice");
        };
        assert!(!text.contains("unauthorized"), "{text}");
        assert_eq!(token, Some(Token::Muted));
        assert_ne!(text, "Loading schema…", "{text}");
    }

    #[test]
    fn a_namespace_and_a_table_are_told_apart_by_colour() {
        let mut state = tabled(&[("shop", &["orders"])]);
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema draws a tree");
        };

        let namespace = rows
            .iter()
            .find(|row| row.kind == RowKind::Schema { limb: None })
            .expect("a namespace");
        let table = rows
            .iter()
            .find(|row| row.kind.is_table())
            .expect("a table");

        assert_eq!(namespace.token, Token::Schema);
        assert_eq!(table.token, Token::Table);
        assert_ne!(
            namespace.token, table.token,
            "the panel's whole job is the namespace/table distinction"
        );
    }

    #[test]
    fn a_system_schema_stays_quiet() {
        let mut state = tabled(&[("pg_catalog", &["tables"])]);
        let schema = state.schema().cloned().expect("loaded");
        state.tables.open(&schema);

        let PanelBody::Tree { rows, .. } = tables_body(&state) else {
            panic!("a loaded schema draws a tree");
        };

        assert!(
            rows.iter().all(|row| row.token == Token::Faint),
            "a system namespace and its tables are the server's, not the user's"
        );
    }

    #[test]
    fn a_tree_names_the_rows_in_range() {
        let mut state = tabled(&[("shop", &["orders"])]);
        state.tables.selection.start(0);

        let PanelBody::Tree { ranges, .. } = tables_body(&state) else {
            panic!("a loaded schema draws a tree");
        };

        assert_eq!(ranges, Ranges::of(crate::selection::Range::new(0, 0)));
    }

    fn on_server(schemas: &[(&str, &[&str])]) -> State {
        let mut catalog = crate::tree::tests_support::schema_of(schemas);
        catalog.server = Some(crate::model::Server {
            current: "lazypg".to_owned(),
            databases: vec!["analytics".to_owned(), "lazypg".to_owned()],
        });
        let mut state = super::super::tests_support::connected(catalog.clone());
        state.tables.seat_on_current_database(&catalog);
        for _ in 0..2 {
            state.tables.go_to(2, &catalog);
            state.tables.open(&catalog);
        }
        state
    }

    fn kinds(state: &State) -> Vec<(String, RowKind, Token)> {
        let PanelBody::Tree { rows, .. } = tables_body(state) else {
            panic!("a loaded catalog draws a tree");
        };
        rows.into_iter()
            .map(|row| (row.label, row.kind, row.token))
            .collect()
    }

    #[test]
    fn under_a_database_schemas_and_tables_carry_the_limbs_that_draw_the_guides() {
        let state = on_server(&[("shop", &["customer", "orders"]), ("zoo", &["animal"])]);

        assert_eq!(
            kinds(&state),
            [
                (
                    "analytics".to_owned(),
                    RowKind::Database { current: false },
                    Token::Muted
                ),
                (
                    "lazypg".to_owned(),
                    RowKind::Database { current: true },
                    Token::Database
                ),
                (
                    "shop".to_owned(),
                    RowKind::Schema {
                        limb: Some(Limb::Middle)
                    },
                    Token::Schema
                ),
                (
                    "customer".to_owned(),
                    RowKind::Table {
                        limb: Limb::Middle,
                        outer: Some(Limb::Middle),
                        relation: RelationKind::Table
                    },
                    Token::Table
                ),
                (
                    "orders".to_owned(),
                    RowKind::Table {
                        limb: Limb::Last,
                        outer: Some(Limb::Middle),
                        relation: RelationKind::Table
                    },
                    Token::Table
                ),
                (
                    "zoo".to_owned(),
                    RowKind::Schema {
                        limb: Some(Limb::Last)
                    },
                    Token::Schema
                ),
            ],
            "zoo is folded; the last schema's limb ends the guide"
        );
    }
}
