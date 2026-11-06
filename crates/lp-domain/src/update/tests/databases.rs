//! The server level of the Tables tree: listing, seating, folding and
//! switching to another database.

use super::*;
use crate::config::ConnectionConfig;
use crate::model::Server;

fn on_server(current: &str) -> Catalog {
    Catalog {
        server: Some(Server {
            current: current.to_owned(),
            databases: ["analytics", "lazypg", "postgres"]
                .map(str::to_owned)
                .to_vec(),
        }),
        ..tabled_schema(&[("public", &[]), ("shop", &["customer", "orders"])])
    }
}

/// Connected as `local`, which the config knows, with the first catalog in.
fn serving(current: &str) -> State {
    let mut state = connected(Posture::Writable);
    state.config.connections = vec![ConnectionConfig {
        host: Some("db".to_owned()),
        posture: Posture::Writable,
        ..ConnectionConfig::named(ConnectionName::try_new("local").expect("a valid name"))
    }];
    let conn = current_conn(&state);
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(on_server(current)),
            took: Duration::ZERO,
        },
    );
    state
}

fn labels(state: &State) -> Vec<String> {
    let schema = state.schema().expect("loaded");
    state
        .tables
        .rows(schema)
        .iter()
        .map(|node| match node {
            Node::Database { name, .. } => (*name).to_owned(),
            Node::Schema { namespace, .. } => format!("  {}", namespace.name),
            Node::Table { table, .. } => format!("    {}", table.name),
        })
        .collect()
}

#[test]
fn every_database_is_listed_and_only_the_current_one_holds_schemas() {
    let state = serving("lazypg");

    assert_eq!(
        labels(&state),
        ["analytics", "lazypg", "  public", "  shop", "postgres"]
    );
}

#[test]
fn the_first_catalog_seats_the_cursor_on_the_database_it_describes() {
    let state = serving("lazypg");

    assert_eq!(
        state.tables.cursor().index(),
        1,
        "the configured database is preselected, not the first listed"
    );
}

#[test]
fn the_current_database_folds_and_unfolds_like_a_schema() {
    let mut state = serving("lazypg");
    state.focus_on(Panel::Objects);

    press(&mut state, Key::Char('h'));
    assert_eq!(labels(&state), ["analytics", "lazypg", "postgres"]);

    press(&mut state, Key::Char('l'));
    assert_eq!(labels(&state).len(), 5);
}

#[test]
fn leaving_a_folded_schema_steps_up_to_its_database() {
    let mut state = serving("lazypg");
    state.focus_on(Panel::Objects);
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));

    press(&mut state, Key::Char('h'));

    assert_eq!(state.tables.cursor().index(), 1, "on lazypg");
}

#[test]
fn opening_another_database_reconnects_the_same_entry_to_it() {
    let mut state = serving("lazypg");
    state.focus_on(Panel::Objects);
    let before = current_conn(&state);
    press(&mut state, Key::Char('k'));

    let effects = press(&mut state, Key::Enter);

    let [Effect::Connect { conn, target, .. }] = effects.as_slice() else {
        panic!("one connect, got {effects:?}");
    };
    assert_ne!(
        *conn, before,
        "a fresh id, so the old session's late results drop"
    );
    assert_eq!(target.name.as_str(), "local");
    assert_eq!(target.database.as_deref(), Some("analytics"));
    assert_eq!(
        target.host.as_deref(),
        Some("db"),
        "the rest of the entry is kept"
    );
    assert!(matches!(
        state.connection,
        ConnectionState::Connecting { .. }
    ));
}

#[test]
fn l_on_another_database_switches_as_enter_does() {
    let mut state = serving("lazypg");
    state.focus_on(Panel::Objects);
    press(&mut state, Key::Char('G'));

    let effects = press(&mut state, Key::Char('l'));

    assert!(matches!(
        effects.as_slice(),
        [Effect::Connect { target, .. }] if target.database.as_deref() == Some("postgres")
    ));
}

#[test]
fn the_switched_to_database_is_seated_on_when_its_catalog_lands() {
    let mut state = serving("lazypg");
    state.focus_on(Panel::Objects);
    press(&mut state, Key::Char('k'));
    let effects = press(&mut state, Key::Enter);
    let Some(Effect::Connect { conn, .. }) = effects.first() else {
        panic!("a connect");
    };
    let conn = *conn;

    update(
        &mut state,
        Msg::Connected {
            conn,
            posture: Posture::Writable,
            took: Duration::ZERO,
        },
    );
    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(on_server("analytics")),
            took: Duration::ZERO,
        },
    );

    assert_eq!(
        labels(&state),
        ["analytics", "  public", "  shop", "lazypg", "postgres"]
    );
    assert_eq!(state.tables.cursor().index(), 0);
}

#[test]
fn a_reread_after_ddl_leaves_the_cursor_where_it_was() {
    let mut state = serving("lazypg");
    state.focus_on(Panel::Objects);
    press(&mut state, Key::Char('j'));
    press(&mut state, Key::Char('j'));
    let conn = current_conn(&state);

    update(
        &mut state,
        Msg::SchemaLoaded {
            conn,
            schema: Box::new(on_server("lazypg")),
            took: Duration::ZERO,
        },
    );

    assert_eq!(state.tables.cursor().index(), 3);
}
