# Developing LazyPg

Tool versions are pinned in [mise.toml](../mise.toml); `mise install`, then
the tasks listed in [AGENTS.md](../AGENTS.md).

## Fixture database

`mise run db:up` starts Postgres on `localhost:5432` (`LP_PG_PORT` moves it),
seeded from `docker/seed` on first start:

- `shop` — keys, composite primary key, foreign keys, indexes, a view
- `zoo` — one column per type family the grid must render
- `"Edge Cases"` — quoted and Unicode identifiers, a keyless table, a
  materialized view, a partitioned table
- a second database, `analytics`, with a `metrics` schema, for switching

`mise run db:psql` opens psql; `mise run db:down` drops the volume, so the
next `db:up` re-seeds. [.lazypg.toml](../.lazypg.toml) points the app at it
with writes enabled.

To exercise cancellation, run `SELECT pg_sleep(30);` and cancel it.

## Releasing

Run the **Prepare release** workflow on `main` with a version such as
`0.1.0`. It requires a green CI run on that commit, bumps
`[workspace.package] version`, tags `v<version>` and dispatches
`release.yml`, which builds with cargo-dist and pushes `lazypg.rb` to
`ike-forge/homebrew-tap` using the `HOMEBREW_TAP_TOKEN` secret.

`release.yml` is generated: change `dist-workspace.toml`, then run
`dist generate`. `dist plan` lists what a release would build.
