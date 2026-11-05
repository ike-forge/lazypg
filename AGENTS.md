# Agent instructions — LazyPg

Read before touching code, in this order:

- [docs/vision.md](docs/vision.md) — why, scope, roadmap
- [docs/architecture.md](docs/architecture.md) — crate boundaries, decisions, open questions
- [docs/plan.md](docs/plan.md) — phases, steps, the LazyCassandra port map
- [docs/conventions.md](docs/conventions.md) — how the code should read

Much of the code is ported from LazyCassandra (`../lazycassandra`). Port with
renames and keep the tests; plan.md says which modules are copy, adapt or
rewrite.

## Non-negotiables

- **No `unsafe`, no FFI.** Enforced by `unsafe_code = "forbid"`.
- **Push invariants into types.** Newtype over validated `String`, enum over
  `bool`, unrepresentable state over a runtime check.
- **Crate boundaries are load-bearing.** `lp-domain` never depends on
  `tokio`, `ratatui`, `crossterm` or a driver.
- **Zero warnings, zero failing or ignored tests.** `mise run ci` clean before
  calling anything done. Any `#[allow]` is item-scoped with a reason.
- **No narrating comments.** Comments are for edge cases, past bugs, upstream
  defects and Postgres semantics the code cannot express.
- **Determinism** in the fast tiers: no wall clock, network, filesystem,
  unseeded randomness, `sleep` or `HashMap` order. Time enters as `Msg::Tick`.
- **Git: commit freely, never `pull`, never `push`.**
- **Conventional Commits**: `<type>[scope]: <description>`, imperative,
  lowercase, no trailing period.

## Workflow

```
mise run build   # cargo build --workspace
mise run fmt     # cargo fmt --all
mise run check   # fmt, clippy -D warnings, cargo deny
mise run test    # fast tiers
mise run ci      # everything CI gates on
mise run db:up   # fixture Postgres, seeded from docker/seed
```
