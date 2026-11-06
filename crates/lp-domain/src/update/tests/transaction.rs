use super::*;
use crate::connection::Transaction;

fn finished(state: &mut State, sql: &str) -> Vec<Effect> {
    let (query, _) = crate::update::submit(state, Statement::typed(sql)).expect("submits");

    update(
        state,
        Msg::PageReady {
            query,
            page: Box::new(Page::default()),
            took: Duration::ZERO,
        },
    )
}

fn reported(state: &mut State, transaction: Transaction) {
    let conn = current_conn(state);
    update(
        state,
        Msg::Transaction {
            conn,
            transaction: Ok(transaction),
        },
    );
}

fn probes(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, Effect::ProbeTransaction(_)))
}

#[test]
fn a_write_asks_the_server_about_the_transaction_and_an_idle_read_does_not() {
    let mut state = connected(Posture::Writable);

    assert!(probes(&finished(&mut state, "BEGIN")));
    assert!(!probes(&finished(&mut state, "SELECT 1")));
}

#[test]
fn a_read_only_connection_never_asks() {
    let mut state = connected(Posture::ReadOnly);

    assert!(!probes(&finished(&mut state, "SELECT 1")));
}

#[test]
fn once_open_every_statement_asks_since_a_commit_reads_as_a_read() {
    let mut state = connected(Posture::Writable);
    reported(&mut state, Transaction::Open);

    assert_eq!(state.transaction, Transaction::Open);
    assert_eq!(
        toasted(&state),
        Some((
            Severity::Info,
            "transaction open, COMMIT or ROLLBACK to end it"
        ))
    );
    assert!(probes(&finished(&mut state, "COMMIT")));
}

#[test]
fn a_report_from_a_connection_since_left_is_dropped() {
    let mut state = connected(Posture::Writable);

    update(
        &mut state,
        Msg::Transaction {
            conn: ConnId::next(),
            transaction: Ok(Transaction::Open),
        },
    );

    assert_eq!(state.transaction, Transaction::Idle);
}

#[test]
fn quitting_inside_a_transaction_asks_first() {
    let mut state = connected(Posture::Writable);
    reported(&mut state, Transaction::Open);

    press(&mut state, Key::Char('q'));

    assert!(!state.should_quit);
    assert!(matches!(&state.overlay, Some(Overlay::Discard(discard))
            if discard.message() == "The open transaction will be rolled back."));
}

#[test]
fn committed_edits_join_an_open_transaction_rather_than_commit_it() {
    let mut state = committable(2);
    reported(&mut state, Transaction::Open);

    ctrl(&mut state, Key::Char('s'));
    let effects = press(&mut state, Key::Char('y'));

    let Some(Effect::Execute { stmt, .. }) = effects
        .iter()
        .find(|effect| matches!(effect, Effect::Execute { .. }))
    else {
        panic!("the commit runs: {effects:?}");
    };
    assert!(!stmt.sql.contains("BEGIN"), "{}", stmt.sql);
    assert!(!stmt.sql.contains("COMMIT"), "{}", stmt.sql);
    assert_eq!(stmt.sql.matches("UPDATE").count(), 2);
}
