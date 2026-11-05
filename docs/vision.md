# LazyPg — Vision

> A terminal UI for PostgreSQL, in the tradition of LazyGit and k9s.

**Last updated:** 2026-11-05

---

## 1. Why

`psql` is excellent and has no sense of place: no navigation, no memory of
where you were, every look at a table is a hand-typed `\d+` or `SELECT`.
GUI clients (DBeaver, pgAdmin) are heavy and mouse-first. Existing TUIs treat
Postgres as a generic SQL endpoint and take their layout from the client, not
from LazyGit.

LazyPg is the Postgres tool you keep open in a split and drive by keyboard.

## 2. The thesis

Postgres forgives modelling mistakes at write time and charges for them at
read time. The query that is instant on a laptop does a sequential scan at
200 million rows; a missing index on a foreign key turns a delete into a
table lock; bloat and an unvacuumed table degrade quietly for weeks.

The facts that explain this — keys, indexes, row estimates, the plan, the
size on disk, who holds which lock — all exist in the catalog and the
statistics views. They are a query away, which in practice means nobody looks
until something breaks.

**Make the structure and cost of your data continuously visible, and make
exploring it fast enough that you actually do it.**

The MVP delivers the first half: structure on screen while you browse.
Plans, explanations, disk usage and live activity (§7) build on it.

## 3. Who it's for

- **The application developer** who needs to see what a table looks like and
  what references it before writing against it.
- **The on-call engineer** who needs to know what is running, what is blocked
  and what is big, right now.
- **The DBA** who wants keyboard-driven everything and no ceremony.

One interface serves all three (§4).

## 4. Interface principles

Carried over from LazyCassandra unchanged; they are the reason this is a
separate product rather than a LazySQL fork.

1. **Discoverability is ambient, not modal.** The bottom bar shows what can
   be done to the focused thing. `?` shows the contextual map.
2. **The same key at every level of skill.** No hidden fast path; depth comes
   from composition (counts, ranges, filters).
3. **Show the SQL. Always.** Every action is expressible as SQL and is shown
   before it runs and logged after. Using LazyPg should make you better at
   Postgres, and you can always copy the statement out.
4. **Panels are places.** Persistent, numbered panels in fixed positions.
   Layout never rearranges itself.
5. **Colour is a relationship.** Themes declare their own ground and are held
   to 4.5:1 contrast by a test.
6. **Destructive things are obvious, reversible, or confirmed — pick two.**
   Read-only by default; writes visibly distinct; drops and truncates need
   typed confirmation.
7. **Speed is a feature of the interface.** The UI thread never touches the
   server. Every query is cancellable.
8. **Density with a floor of legibility.** Every dense view is labelled;
   every abbreviation expands.

## 5. Layout

Four panels, mirroring LazyCassandra:

| # | Panel | Holds |
|---|---|---|
| 1 | Objects | schema → tables, views, materialized views, sequences, functions |
| 2 | Structure | columns, keys and foreign keys (both directions), indexes, constraints, DDL |
| 3 | Contents | the result grid: table rows or query results, paged |
| 4 | Queries | SQL editor and session log |

## 6. Configuration

`.lazypg.toml` in the project root, layered over
`$XDG_CONFIG_HOME/lazypg/config.toml`. The project file is meant to be
committed. Credentials are literal, environment or file references; nothing
in the config path executes a command. `~/.pgpass` and `PG*` environment
variables are honoured as credential sources.

Connections carry a posture — read-only, writable or protected — shown at
all times. Read-only is also enforced server-side
(`default_transaction_read_only`), so a misclassified statement is refused by
Postgres rather than executed.

## 7. Roadmap

Phases, not dates. Each phase is useful on its own. Detail in
[plan.md](plan.md).

- **Phase 0 — Foundations.** The LazyCassandra shell ported, Postgres driver,
  config, schema introspection.
- **Phase 1 — Browse (MVP).** Object tree, structure panel, paged grid, SQL
  editor with completion, session log, history, inspect, yank, export.
- **Phase 2 — Change.** Cell edits, insert/delete by primary key, drops and
  truncates behind confirmation, explicit transactions.
- **Phase 3 — Understand.** `EXPLAIN` / `EXPLAIN ANALYZE` as a navigable plan
  tree, plan node explanations, estimate-vs-actual flags, missing-index hints.
- **Phase 4 — Operate.** Disk usage (relation, index, TOAST, bloat
  estimates), `pg_stat_activity`, locks and blocking chains,
  `pg_stat_statements`, vacuum and analyze state, cancel/terminate backend.
- **Phase 5 — Extend.** User keybindings, saved queries.

Supported servers: PostgreSQL 14 and later.

## 8. Non-goals

- Not a GUI or web app.
- Not a multi-database client. Postgres is the model; MySQL and others would
  be their own products sharing the shell, if ever.
- Not a migration, ETL or backup tool.
- Not a monitoring system: it shows live state, it does not store history or
  alert.
