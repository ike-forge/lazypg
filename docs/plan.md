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
   the scylla adapter, its TLS setup and node health reads removed. The app
   runs against `lp_outbound::demo`. Domain and config keep the Cassandra
   vocabulary (`Keyspace`, `Cluster`, `hosts`) until steps 3–4 and Phase 1
   replace it; every step keeps the app running and the tests green.
2. **Driver.** `tokio-postgres` + rustls against `docker/`: connect,
   cancel a `pg_sleep`, page with `LIMIT/OFFSET`, decode every
   `zoo.everything` column. TLS from LazyCassandra's
   `lc-outbound/src/tls.rs` via `tokio-postgres-rustls`. Record the
   `deny.toml` cost in architecture.md §8.
3. **Config.** Port config layering; Postgres connection fields; pgpass and
   `PG*`; `sslmode`. Connection picker works.
4. **Schema model and introspection.** Accessible databases, then per
   database `pg_catalog` queries for schemas,
   relations, columns, keys, foreign keys, indexes, constraints, sequences,
   functions. Integration tests against the seed, including the
   `"Edge Cases"` schema.
5. **Fixture.** Pin the image by digest (conventions.md §4); add a
   Postgres 14 service and a second seeded database for the tree.

## Phase 1 — Browse (MVP)

1. **Objects panel.** Database → schema → relation kinds tree, the
   connection's database unfolded on start, a connection per database
   opened on unfold, system schemas hidden by default, partitions folded
   under their parent.
2. **Structure panel.** Tabs: Columns (type, nullability, default, PK
   marker), Keys (PK, outgoing and incoming FKs), Indexes, Constraints, DDL
   (reconstructed `CREATE`, with `pg_get_*def` where Postgres provides one).
3. **Contents grid.** `LIMIT/OFFSET` paging (architecture.md §6); `PgValue`
   rendering for every type family; NULL distinct from empty; local filter
   and sort.
4. **SQL editor.** Lexer, highlighting, statement split under the cursor,
   run, cancel, `$EDITOR` hand-off, schema-aware completion (schemas,
   relations, columns, keywords).
5. **Session log and history.** Every statement LazyPg runs, including its
   own catalog queries when asked to show them.
6. **Inspect, yank, export.** Cell inspector (pretty JSON, arrays,
   composites), yank as value/row/`INSERT`, CSV and JSON export.
7. **Posture.** Statement classification plus server-side read-only session.
8. **Release.** README, screenshots, cargo-dist, Homebrew tap.

## Phase 2 — Change

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

## Open questions

- Connection budget: one per unfolded database, more once portals pin one
  per open result. Cap and reuse policy undecided.
