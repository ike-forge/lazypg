//! One database's catalog: schemas, their relations, and what keys and
//! references them, made structural so the panels never re-derive it
//! (vision.md §2).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalog {
    /// Ordered so iteration is deterministic.
    pub schemas: BTreeMap<String, Schema>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub name: String,
    /// Every relation kind; [`Table::kind`] tells them apart.
    pub tables: BTreeMap<String, Table>,
    pub sequences: Vec<Sequence>,
    pub functions: Vec<Function>,
}

impl Schema {
    #[must_use]
    pub fn empty(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            tables: BTreeMap::new(),
            sequences: Vec::new(),
            functions: Vec::new(),
        }
    }
}

/// `pg_class.relkind`, for the kinds that hold or present rows. Partitions are
/// a [`Table`]'s `partitions`, not relations of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RelationKind {
    #[default]
    Table,
    Partitioned,
    View,
    MaterializedView,
    Foreign,
}

impl RelationKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Table => "Table",
            Self::Partitioned => "Partitioned table",
            Self::View => "View",
            Self::MaterializedView => "Materialized view",
            Self::Foreign => "Foreign table",
        }
    }

    /// Only these accept `INSERT`, `UPDATE` and `DELETE` without rules or
    /// triggers.
    #[must_use]
    pub const fn stores_rows(self) -> bool {
        matches!(self, Self::Table | Self::Partitioned)
    }
}

/// A relation of any [`RelationKind`]: the name Postgres's own docs and
/// `\d` use loosely, and the one the panels already speak.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Table {
    pub name: String,
    pub kind: RelationKind,
    /// In `attnum` order: what `SELECT *` returns.
    pub columns: Vec<Column>,
    pub primary_key: Option<PrimaryKey>,
    /// Outgoing; the incoming ones are [`Catalog::referencing`].
    pub foreign_keys: Vec<ForeignKey>,
    pub indexes: Vec<Index>,
    /// Check, unique and exclusion: the primary and foreign keys have their
    /// own fields.
    pub constraints: Vec<Constraint>,
    pub partitions: Vec<String>,
    pub comment: Option<String>,
    /// A view's `SELECT`, as `pg_get_viewdef` renders it.
    pub definition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// As `format_type` renders it, e.g. `character varying(16)`.
    pub data_type: String,
    pub nullable: bool,
    /// The expression, or the identity/generation clause.
    pub default: Option<String>,
    pub comment: Option<String>,
}

impl Column {
    #[must_use]
    pub fn new(name: &str, data_type: &str) -> Self {
        Self {
            name: name.to_owned(),
            data_type: data_type.to_owned(),
            nullable: true,
            default: None,
            comment: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryKey {
    pub name: String,
    /// In key order, which need not be column order. Never empty.
    pub columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub target: RelationRef,
    /// Paired with `columns` by position.
    pub target_columns: Vec<String>,
    /// SQL's words: `NO ACTION`, `CASCADE`, …
    pub on_update: String,
    pub on_delete: String,
}

/// Unquoted names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelationRef {
    pub schema: String,
    pub table: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    pub name: String,
    /// `pg_get_indexdef`: the whole `CREATE INDEX`.
    pub definition: String,
    /// Column names, or the expression text for an expression key.
    pub keys: Vec<String>,
    pub unique: bool,
    /// Backs the primary key.
    pub primary: bool,
    /// `btree`, `gin`, …
    pub method: String,
    pub predicate: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintKind {
    Check,
    Unique,
    Exclusion,
}

impl ConstraintKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Unique => "unique",
            Self::Exclusion => "exclusion",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    pub name: String,
    pub kind: ConstraintKind,
    /// `pg_get_constraintdef`, e.g. `CHECK ((price >= (0)::numeric))`.
    pub definition: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub name: String,
    pub data_type: String,
    /// `table.column`, for a serial or identity column's sequence.
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    Function,
    Procedure,
    Aggregate,
    Window,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub kind: FunctionKind,
    /// `pg_get_function_identity_arguments`: tells overloads apart.
    pub arguments: String,
    /// Empty for a procedure.
    pub result: String,
}

impl Table {
    pub fn columns(&self) -> impl Iterator<Item = &Column> {
        self.columns.iter()
    }

    #[must_use]
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|column| column.name == name)
    }

    /// Empty without a primary key.
    pub fn key_columns(&self) -> impl Iterator<Item = &Column> {
        self.primary_key
            .iter()
            .flat_map(|key| key.columns.iter())
            .filter_map(|name| self.column(name))
    }

    #[must_use]
    pub fn is_key(&self, column: &str) -> bool {
        self.primary_key
            .as_ref()
            .is_some_and(|key| key.columns.iter().any(|name| name == column))
    }

    /// Whether `column` is among some foreign key's columns.
    #[must_use]
    pub fn references(&self, column: &str) -> bool {
        self.foreign_keys
            .iter()
            .any(|key| key.columns.iter().any(|name| name == column))
    }
}

/// Derived from the name, so an adapter cannot report a contradicting kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchemaKind {
    User,
    System,
}

impl SchemaKind {
    /// `pg_catalog`, `information_schema` and the `pg_` reserved prefix, which
    /// `CREATE SCHEMA` refuses to users.
    #[must_use]
    pub fn of(name: &str) -> Self {
        if name == "information_schema" || name.starts_with("pg_") {
            Self::System
        } else {
            Self::User
        }
    }
}

impl Schema {
    #[must_use]
    pub fn kind(&self) -> SchemaKind {
        SchemaKind::of(&self.name)
    }
}

impl Catalog {
    /// User schemas first, then the system's, each alphabetical.
    pub fn schemas_in_display_order(&self) -> impl Iterator<Item = &Schema> {
        let user = self
            .schemas
            .values()
            .filter(|schema| schema.kind() == SchemaKind::User);
        let system = self
            .schemas
            .values()
            .filter(|schema| schema.kind() == SchemaKind::System);

        user.chain(system)
    }

    #[must_use]
    pub fn table(&self, at: &RelationRef) -> Option<&Table> {
        self.schemas.get(&at.schema)?.tables.get(&at.table)
    }

    /// The foreign keys pointing at `target`, with the relation each is on.
    pub fn referencing<'a>(
        &'a self,
        target: &'a RelationRef,
    ) -> impl Iterator<Item = (RelationRef, &'a ForeignKey)> + 'a {
        self.schemas.values().flat_map(move |schema| {
            schema.tables.values().flat_map(move |table| {
                table
                    .foreign_keys
                    .iter()
                    .filter(move |key| key.target == *target)
                    .map(move |key| {
                        (
                            RelationRef {
                                schema: schema.name.clone(),
                                table: table.name.clone(),
                            },
                            key,
                        )
                    })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(schema: &str, table: &str) -> RelationRef {
        RelationRef {
            schema: schema.to_owned(),
            table: table.to_owned(),
        }
    }

    fn order_line() -> Table {
        Table {
            name: "order_line".to_owned(),
            columns: vec![
                Column::new("order_id", "bigint"),
                Column::new("line_no", "integer"),
                Column::new("customer_id", "bigint"),
            ],
            primary_key: Some(PrimaryKey {
                name: "order_line_pkey".to_owned(),
                columns: vec!["line_no".to_owned(), "order_id".to_owned()],
            }),
            foreign_keys: vec![ForeignKey {
                name: "order_line_customer_id_fkey".to_owned(),
                columns: vec!["customer_id".to_owned()],
                target: at("shop", "customer"),
                target_columns: vec!["id".to_owned()],
                on_update: "NO ACTION".to_owned(),
                on_delete: "CASCADE".to_owned(),
            }],
            ..Table::default()
        }
    }

    fn catalog() -> Catalog {
        let mut shop = Schema::empty("shop");
        shop.tables.insert("order_line".to_owned(), order_line());
        shop.tables.insert(
            "customer".to_owned(),
            Table {
                name: "customer".to_owned(),
                ..Table::default()
            },
        );
        let mut catalog = Catalog::default();
        catalog.schemas.insert("shop".to_owned(), shop);
        catalog
            .schemas
            .insert("pg_catalog".to_owned(), Schema::empty("pg_catalog"));
        catalog
            .schemas
            .insert("public".to_owned(), Schema::empty("public"));
        catalog
    }

    #[test]
    fn key_columns_follow_key_order_not_column_order() {
        let names: Vec<_> = order_line().key_columns().map(|c| c.name.clone()).collect();
        assert_eq!(names, ["line_no", "order_id"]);
    }

    #[test]
    fn a_keyless_table_has_no_key_columns_and_no_column_is_key() {
        let keyless = Table {
            primary_key: None,
            ..order_line()
        };
        assert_eq!(keyless.key_columns().count(), 0);
        assert!(!keyless.is_key("order_id"));
    }

    #[test]
    fn a_column_is_key_or_references_by_the_constraints_it_is_in() {
        let table = order_line();
        assert!(table.is_key("order_id"));
        assert!(!table.is_key("customer_id"));
        assert!(table.references("customer_id"));
        assert!(!table.references("line_no"));
    }

    #[test]
    fn the_foreign_keys_into_a_table_are_found_from_the_other_side() {
        let catalog = catalog();
        let target = at("shop", "customer");
        let incoming: Vec<_> = catalog
            .referencing(&target)
            .map(|(from, key)| (from.table, key.name.clone()))
            .collect();

        assert_eq!(
            incoming,
            [(
                "order_line".to_owned(),
                "order_line_customer_id_fkey".to_owned()
            )]
        );
        assert_eq!(catalog.referencing(&at("shop", "order_line")).count(), 0);
    }

    #[test]
    fn the_reserved_prefix_and_information_schema_are_the_systems() {
        assert_eq!(SchemaKind::of("pg_catalog"), SchemaKind::System);
        assert_eq!(SchemaKind::of("pg_toast"), SchemaKind::System);
        assert_eq!(SchemaKind::of("information_schema"), SchemaKind::System);
        assert_eq!(SchemaKind::of("public"), SchemaKind::User);
        assert_eq!(SchemaKind::of("pgx"), SchemaKind::User);
    }

    #[test]
    fn display_order_puts_user_schemas_before_the_systems() {
        let names: Vec<_> = catalog()
            .schemas_in_display_order()
            .map(|schema| schema.name.clone())
            .collect();
        assert_eq!(names, ["public", "shop", "pg_catalog"]);
    }

    #[test]
    fn a_relation_is_found_by_reference() {
        assert!(catalog().table(&at("shop", "customer")).is_some());
        assert!(catalog().table(&at("shop", "nope")).is_none());
    }
}
