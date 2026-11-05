//! The Postgres vocabulary in our terms; driver types stop at the adapter
//! (architecture.md §5).

pub mod error;
pub mod ids;
pub mod input;
pub mod page;
pub mod schema;
pub mod statement;
pub mod value;

pub use error::DbError;
pub use ids::{ConnId, JobId, QueryId};
pub use input::{Key, KeyPress, Modifiers};
pub use page::{Page, PagingState, Row};
pub use schema::{
    Catalog, Column, Constraint, ConstraintKind, ForeignKey, Function, FunctionKind, Index,
    PrimaryKey, RelationKind, RelationRef, Schema, SchemaKind, Sequence, Server, Table,
};
pub use statement::{Statement, StatementKind, UpdatedRow};
pub use value::CqlValue;
