//! The one connection and how far along it is.

use crate::config::ConnectionName;
use crate::model::{Catalog, ConnId, DbError};
use crate::posture::Posture;

/// `Option<Connection>` could not say "connecting" or "failed".
#[derive(Debug, Clone, Default)]
pub enum ConnectionState {
    #[default]
    Disconnected,
    /// Unlike [`Self::Disconnected`], the picker is still owed: diagnostics can
    /// show first, and a dismissed picker stays away.
    Unchosen,
    /// The id matches a failure to the outstanding attempt; the name saves a
    /// config search later.
    Connecting {
        conn: ConnId,
        name: ConnectionName,
    },
    Failed(DbError),
    Connected(Connection),
}

#[derive(Debug, Clone)]
pub struct Connection {
    pub id: ConnId,
    /// Held: the live connection is not a row of the config.
    pub name: ConnectionName,
    pub posture: Posture,
    pub schema: SchemaState,
}

#[derive(Debug, Clone, Default)]
pub enum SchemaState {
    #[default]
    Loading,
    Failed(DbError),
    Loaded(Catalog),
}

impl ConnectionState {
    /// Drops results from a connection the user has left.
    #[must_use]
    pub fn is_current(&self, conn: ConnId) -> bool {
        match self {
            Self::Connecting { conn: id, .. } => *id == conn,
            Self::Connected(connection) => connection.id == conn,
            Self::Unchosen | Self::Disconnected | Self::Failed(_) => false,
        }
    }
}

/// The session's transaction block, as the server last reported it. One
/// client serves every request, so a `BEGIN` the user typed holds the grid's
/// reads and writes too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transaction {
    #[default]
    Idle,
    Open,
    /// Aborted by an error: every statement but `ROLLBACK` is refused.
    Failed,
}
