# LazyPg — Plan

Phases from [vision.md](vision.md) §7, broken into steps. Each step ends with
`mise run ci` green and a commit.

Source paths are relative to the LazyCassandra repository (`lc-*` crates).
Ported code is renamed `lc_` → `lp_` and keeps its tests.

**Last updated:** 2026-11-05

---

## Port map

Every LazyCassandra module falls into one of three buckets.

**Copy** — database-neutral; port with renames only.

- domain: `viewport`, `selection`, `geometry`, `overlay`, `panel`, `line`,
  `pointer`, `feedback`, `jobs`, `clock`, `bytes`, `log`, `search`,
  `editor`, `history`, `connection`, `ports` (renamed `Cluster` →
  `Database`), `posture`, `view/title`, `view/logs`, `contents/filter`,
  `update/{palette,search,history,pointer,themes,visual,yank}`
- inbound: `layout`, `render`, `hit`, `hints`, `footer`, `toast`, `loader`,
  `overlay`, `palette`, `filter`, `input`, `logs`, `history`, `export`,
  `commit`, `themes`, `theme/*`, `too_small`, `wrap`, `highlight`
- outbound: `clock`, `file`, `buffer`, `tls`, `history`, `config/{save,merge,discover}`
- app: all of it

**Adapt** — the shape carries over, the vocabulary changes.

- domain: `state`, `commands`, `key`, `style`, `theme` (tokens:
  `PrimaryKey`, `ForeignKey`, `Index`… replace partition/clustering),
  `config`, `tree`, `ddl` (becomes Structure), `contents/*`, `grid`, `cell`,
  `literal`, `yank`, `export`, `inspect`, `edit`, `commit`, `drop`,
  `queries`, `view/*`, `update/*` not listed above
- inbound: `body`, `details`, `inspect`, `drop`, `connections`, `icons`,
  `diagnostics`
- outbound: `config/{mod,wire,validate,example}`, `secret` (+ pgpass),
  `fake`, `value`

**Rewrite or drop**

- `cql/` → `sql/`: lexer (dollar quoting, `E''` strings, quoted identifiers,
  `::` casts), statement classification, resolver, completion, formatter
- `model/` → Postgres schema model (architecture.md §4)
- `model/class_option.rs`, `health` (domain, inbound, outbound): dropped
  until Phase 4
- `outbound/cassandra.rs` → `postgres.rs`

## Phase 0 — Foundations

1. **Shell port.** *Done.* The whole LazyCassandra workspace, renamed, with
   the scylla adapter, its TLS setup and node health reads removed. Some
   Cassandra vocabulary remains (the `Cluster` port, `keyspace` locals, the
   DDL panel's name, CQL in `cql/`); Phase 1 replaces it as it touches each
   area.
2. **Driver.** *Done.* `tokio-postgres` + rustls against `docker/`: connect,
   cancel a `pg_sleep`, page with `LIMIT/OFFSET`, decode every
   `zoo.everything` column. TLS from LazyCassandra's
   `lc-outbound/src/tls.rs` via `tokio-postgres-rustls`. Record the
   `deny.toml` cost in architecture.md §8.
3. **Config.** *Done.* libpq's connection keys (`host`, `port`, `database`,
   `user`, `password`, `sslmode`, `sslrootcert`, `sslcert`, `sslkey`) or a
   `url`, JDBC accepted. Unstated keys fall back to `PG*`, then libpq's
   defaults; the password to `PGPASSWORD`, then the passfile.
4. **Schema model and introspection.** *Done, but for listing databases,
   which moves to Phase 1 step 1 with the tree.* Per database `pg_catalog`
   queries for schemas,
   relations, columns, keys, foreign keys, indexes, constraints, sequences,
   functions. Integration tests against the seed, including the
   `"Edge Cases"` schema.
5. **Fixture.** Pin the image by digest (conventions.md §4); add a
   Postgres 14 service and a second seeded database for the tree.

## Phase 1 — Browse (MVP)

1. **Objects panel.** *Done.* Database → schema → relation tree, the
   configured database open and preselected on start; opening another
   reconnects to it (architecture.md §7). System schemas are not read;
   partitions are listed under their parent in the Structure panel. Each
   relation kind has its own icon.
2. **Structure panel.** *Done.* Tabs: Columns (type, `not null`, default,
   key and reference markers; comment and partitions beneath), Keys (primary
   key, foreign keys out and in, unique/check/exclusion), Indexes, DDL (a
   `CREATE` rebuilt from the catalog, with indexes and comments). A schema or
   database has one tab, named for the panel. Constraints share Keys: a fifth
   tab would not fit the border at usual widths.
3. **Contents grid.** *Done.* `LIMIT/OFFSET` paging, table reads ordered by
   the primary key; a keyless table past its first page says its pages may
   shift. Values: `numeric` exact, floats and `bytea` as Postgres spells
   them, NULL drawn apart from text; every other type in Postgres's own
   text. The local sort compares numbers as numbers and keeps nulls last.
   Arrays, ranges and composites stay text until the inspector (step 6).
4. **SQL editor.** *Done.* Postgres lexer (`E''` and prefixed strings,
   `$tag$` bodies, nested comments, `$1`, `::`, PG16 numbers), keywords that
   leave common column names (`type`, `key`, `comment`) as names, statement
   splitting with no batch, classification by every word (see
   architecture.md §4), a formatter for joins, set operations and
   `RETURNING`, and a resolver that reads `JOIN … ON`, aliases, CTEs and
   `EXPLAIN`. Multi-row writes are one `BEGIN; …; COMMIT;`. Still to do:
   completing `alias.` to that table's columns, and a CTE's columns.
5. **Session log and history.** *Done.* Every statement run is logged and
   kept per connection. The catalog queries are data in
   `lp_domain::introspection`, so `[log] catalog = true` lists each one as
   sent; by default they are one "Reading schema" entry.
6. **Inspect, yank, export.** *Done.* Arrays (`Value::Array`) and `json`/`jsonb`
   (`Value::Json`) keep Postgres's text in the grid; the inspector lays an
   array out an element a line and re-indents JSON without reordering keys;
   JSON export writes arrays as arrays of their elements' text and embeds
   JSON as the value it is. Composites and ranges stay text. Yanking a row as
   an `INSERT` moves to Phase 2 with the literals it needs.
7. **Posture.** Statement classification plus server-side read-only session.
8. **Release.** README, screenshots, cargo-dist, Homebrew tap.

## Phase 2 — Change

Replaces LazyCassandra's CQL write path: literals (`literal.rs`) and cell
parsing (`edit.rs`), plus the collection and millisecond-timestamp `Value`
variants nothing reads any more. With Postgres literals, yank a row as an
`INSERT`.
Cell edit, row insert and delete (primary key required, shown as `UPDATE …
WHERE pk = …` before commit); explicit transaction mode with a visible open
transaction; drop and truncate behind typed confirmation.

## Phase 3 — Understand

`EXPLAIN (FORMAT JSON)` and `EXPLAIN (ANALYZE, BUFFERS)` (the latter only
for read-only statements or inside a rolled-back transaction) rendered as a
navigable tree; per-node cost, rows estimated vs. actual, time share;
plain-language explanation of each node type; flags for seq scans on large
relations, misestimates, sorts spilling to disk, FKs without an index.

## Phase 4 — Operate

Disk usage per relation (heap, indexes, TOAST, bloat estimate); live
activity from `pg_stat_activity` with cancel/terminate; lock tree and
blocking chains; `pg_stat_statements` top queries; vacuum and analyze
status, dead tuples, wraparound age.

## Future — wanted

- **Exact paging.** Keyset paging on the primary key for table browse;
  portals (`Bind` with `max_rows`) in a `READ ONLY` transaction for ad-hoc
  queries, with an idle timeout and a connection per open result. Replaces
  `LIMIT/OFFSET` (architecture.md §6), which stays as the keyless fallback.

- **Several databases open at once**, each with its own session, so
  catalogs and results from two databases sit side by side.

## Open questions

- Connection budget, once portals pin a connection per open result and
  several databases are open: cap and reuse policy undecided.
