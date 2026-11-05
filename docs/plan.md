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

1. **Shell port.** Copy bucket into the workspace with a stub `Database`
   fake and a placeholder schema. The app launches, panels draw, themes,
   palette, logs and help work against the fake. Workspace deps as
   LazyCassandra minus `scylla`, `bigdecimal`, `num-bigint`.
2. **Driver spike.** `tokio-postgres` + rustls against `docker/`: connect,
   cancel a `pg_sleep`, page with a portal, decode every `zoo.everything`
   column. Record the outcome and the `deny.toml` cost in architecture.md §8,
   and settle paging (§6).
3. **Config.** Port config layering; Postgres connection fields; pgpass and
   `PG*`; `sslmode`. Connection picker works.
4. **Schema model and introspection.** `pg_catalog` queries for schemas,
   relations, columns, keys, foreign keys, indexes, constraints, sequences,
   functions. Integration tests against the seed, including the
   `"Edge Cases"` schema.
5. **Pin the fixture image by digest** (conventions.md §4).

## Phase 1 — Browse (MVP)

1. **Objects panel.** Schema → relation kinds tree, system schemas hidden by
   default, partitions folded under their parent.
2. **Structure panel.** Tabs: Columns (type, nullability, default, PK
   marker), Keys (PK, outgoing and incoming FKs), Indexes, Constraints, DDL
   (reconstructed `CREATE`, with `pg_get_*def` where Postgres provides one).
3. **Contents grid.** Paged table browse per the paging decision; `PgValue`
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

## Open questions

- Paging strategy (architecture.md §6).
- One connection per open result vs. a pool: portals pin a connection.
- Whether the Objects tree roots at the server (list `pg_database`) or the
  connection's database.
- Minimum supported Postgres version. Proposal: 14+, tested on 14 and 18.
