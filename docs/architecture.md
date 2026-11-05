# LazyPg — Architecture

Companion to [vision.md](vision.md). The structure is LazyCassandra's; this
document records what is the same, what differs for Postgres, and what is
still open.

**Status:** Phase 0 steps 1–2 done: LazyCassandra shell ported and running
against Postgres through the Cassandra-shaped domain model.
**Last updated:** 2026-11-05

---

## 1. Decisions

| Decision | Choice | Why |
|---|---|---|
| Language | Rust | Latency floor, single static binary, type-safe long-lived state |
| TUI | `ratatui` + `crossterm` | Same stack as LazyCassandra; the shell ports unchanged |
| State model | Elm-style (TEA) | Pure `update`, headless tests, async results as messages |
| Structure | Hexagonal — domain / inbound / outbound / app | Compiler-enforced boundaries |
| Async runtime | `tokio` | Required by the driver |
| Driver | `tokio-postgres` (§8) | Pure Rust, cancel tokens, portals |
| Server versions | Postgres 14+ | Integration tier runs 14 and 18 |
| Paging (MVP) | `LIMIT/OFFSET` (§6) | One strategy; keyset and portals are wanted later |
| Tree root | The server's databases (§7) | Matches how people think about a server |
| Shared code | Copied from LazyCassandra, not a shared crate | §9 |

## 2. Shape

```
crates/
  lp-domain/    # pure core. std, serde, thiserror, async-trait, unicode-*
  lp-inbound/   # TUI adapter. lp-domain, ratatui, crossterm
  lp-outbound/  # Postgres, config, secrets, history store. lp-domain, tokio, driver
  lp-app/       # binary `lazypg`: composition root, effect runtime, main loop
```

`lp-domain` lists no terminal, runtime or driver; the boundary fails to
compile rather than relying on review. `lp-inbound` and `lp-outbound` never
see each other.

## 3. The loop

Identical to LazyCassandra architecture.md §3: `update(&mut State, Msg) ->
Vec<Effect>`, effects interpreted by `lp-app`, every async result re-entering
as a `Msg`, frame geometry the one backwards edge, messages batched per frame.

## 4. Domain model — what changes

| LazyCassandra | LazyPg |
|---|---|
| `Keyspace → Table` | `Schema → Relation` (`Table`, `View`, `MatView`, `Partitioned`, `Foreign`) plus `Sequence`, `Function` |
| `PrimaryKey { partition, clustering }` | `PrimaryKey(Vec<ColumnId>)`, optional: a table without one is representable and blocks row edits by type |
| — | `ForeignKey { from, to, on_delete, on_update }`, indexed both directions so Structure shows referencing tables |
| — | `Index { columns/expressions, unique, partial predicate, method }`, `Constraint` (check, unique, exclusion) |
| `CqlValue` | `PgValue`: scalar families plus arrays, composites, ranges, enums, `json`/`jsonb`; unknown OIDs fall back to text |
| `Statement` (CQL) | `Statement` (SQL): text plus a `StatementKind` from the lexer, used for posture and the log |
| `PagingState` | page offset — see §6 |
| table health | dropped from the MVP; returns as Phase 4 disk usage and activity |

**Posture gates effects in the domain**, as before: a statement classified
as mutating on a read-only connection never becomes an `Effect`. Postgres
adds a second rail: read-only sessions set `default_transaction_read_only =
on`, so a CTE with a `DELETE` the lexer misses is refused by the server.

## 5. Ports

| Port | Responsibility |
|---|---|
| `Database` | connect, introspect schema, execute, execute paged, cancel |
| `ConfigSource` / `ConfigSink` | as LazyCassandra |
| `SecretResolver` | literal / env / file, plus `~/.pgpass` |
| `Store` | query history |
| `Writer` | export sinks |

`Database` speaks our vocabulary; driver types stop at `lp-outbound`.

## 6. Paging — decided for the MVP

Postgres has no resumable paging state. The MVP uses `LIMIT/OFFSET` for
everything:

- Table browse orders by the primary key when there is one, so pages are
  stable; keyless tables page in heap order.
- A `SELECT`, `VALUES`, `TABLE` or read-only `WITH` is wrapped as
  `SELECT * FROM (<stmt>) q LIMIT n OFFSET m`; anything else runs unwrapped.
- Each page re-runs the query. Deep pages are slow and concurrent writes can
  shift rows between pages; the grid header says it is offset-paged.

**Wanted after the MVP** (plan.md, Future): the strategies below, chosen per
read. `Page` carries an opaque cursor from the start so the switch stays in
`lp-outbound`.


- **Keyset** on the primary key for table browsing (`WHERE pk > $last ORDER
  BY pk LIMIT n`). Stateless, cheap, stable. Needs a primary key.
- **Portal** (`Bind` with `max_rows`) inside a `READ ONLY` transaction for
  ad-hoc queries. Exact and works for any query, but holds a snapshot and a
  connection while the user reads; needs an idle timeout and a dedicated
  connection per open result.
- **`LIMIT/OFFSET`** as the fallback for keyless tables. Simple; degrades on
  deep pages and is unstable under concurrent writes.


## 7. Configuration

As LazyCassandra architecture.md §7, with Postgres connection fields (`host`,
`port`, `database`, `user`, `sslmode`, `application_name`) and `pgpass` /
`PG*` as credential sources.

The Objects tree roots at the server and lists every database the user can
connect to (`pg_database` with `datallowconn` and `CONNECT` privilege). A
connection's `database`, when set, is unfolded and focused on start; when
unset, the session connects to `postgres` for the listing. Postgres scopes a
session to one database, so each unfolded database gets its own connection,
opened on unfold.

## 8. Driver — decided

| Candidate | Assessment |
|---|---|
| `tokio-postgres` | **Chosen.** Pure Rust; `CancelToken` maps onto `Effect::Cancel`; portals for paging; raw row access for unknown types; TLS via `tokio-postgres-rustls` |
| `sqlx` | Compile-time-checked queries are irrelevant for ad-hoc SQL; less direct control over portals and cancellation |
| `libpq` bindings | FFI; ruled out by conventions.md §1 |

Rows are read over the simple query protocol, so every value arrives as
Postgres's own text rendering; a `prepare` of the same text supplies column
types for booleans, integers, floats, `bytea` and `uuid`, and everything else
stays text. `numeric` stays text deliberately. Cancellation is a drop guard:
aborting the task, or a request timeout, sends a cancel request.

Wiring it in cost two `deny.toml` skip entries (`const-oid` 0.9, `wasi` 0.11)
and two licences, `ISC` and `BSD-3-Clause`, for the rustls/aws-lc stack.

## 9. Shared code with LazyCassandra

The TUI shell is copied, not shared. A common crate now would force a
database-neutral domain before the second user exists, and LazyCassandra's
domain is deliberately Cassandra-shaped. Once both have stabilised, candidates
for extraction are the modules plan.md marks as *copy*: viewport, selection,
geometry, editor, theme and palettes, hint bar, log, search, export sinks.

## 10. Testing

The four tiers of LazyCassandra architecture.md §10: domain unit, scenario
(fake `Database`), render (`TestBackend`), integration (the `docker/`
fixture, `--features integration`).

## 11. Conventions

Structural rules as LazyCassandra architecture.md §12: `thiserror` in the
domain, `anyhow` only in `lp-app`, no `unwrap` outside tests and the
composition root, public `lp-domain` items documented, zero warnings.
