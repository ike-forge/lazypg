# LazyPg

A terminal UI for PostgreSQL, in the tradition of LazyGit and k9s: browse
schemas and tables, see keys, foreign keys and indexes next to the rows, and
run SQL without leaving the terminal.

Pre-release. See [docs/vision.md](docs/vision.md) for the scope and
[docs/plan.md](docs/plan.md) for progress.

## Development

```sh
mise install
mise run ci       # build, lint, test
mise run db:up    # fixture Postgres on localhost:5432
mise run run      # launch against it (.lazypg.toml)
```

## Licence

0BSD.
