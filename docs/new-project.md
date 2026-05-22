# Starting a new project with monad

You're about to create a polyglot monorepo and want monad as the orchestrator from day one. This guide takes you from `mkdir` to deployable in 10 minutes.

The matching adoption walkthrough (existing repo) is at [adopt-existing-repo.md](./adopt-existing-repo.md). For complete config detail see [configuration.md](./configuration.md). For the CLI itself: `monad --help`.

## 0. Prerequisites

- monad installed (see [README › Install](../README.md#install))
- A native toolchain for whatever languages you plan to ship. **monad can install the toolchain itself** when you pin a version in `[toolchain]` — Go, Node, and Python (via `uv`) are auto-installed into `~/.monad/tools/` on demand. For other languages (Java, Ruby, PHP, …) monad uses whatever's on `$PATH`. See [README › Toolchain handling](../README.md#toolchain-handling) for the full opt-in / opt-out semantics.

## 1. Bootstrap the workspace

```console
$ mkdir myapp && cd myapp
$ git init
$ monad init
✓ initialised monad workspace at /home/you/myapp

files:
  monad.toml
  profiles/release.toml

next:
  monad unit add apps/api --lang go
  monad plan
```

You now have:

```
myapp/
├── monad.toml          # repo-wide defaults; tweak only what you care about
└── profiles/
    └── release.toml    # name = "release", units = []
```

`init` in an empty dir creates the placeholders only — there's nothing to detect yet. As you add units, they get wired in automatically.

If you'd rather your monad be called something other than `release` — `backend`, `core`, anything — rename now (`mv profiles/release.toml profiles/<name>.toml` and edit the `name` field inside). You can have multiple profiles for different deployment groupings; see [README › Vocabulary](../README.md#vocabulary) and [configuration.md › Multiple profiles](./configuration.md#multiple-profiles).

## 2. Add your first unit

`monad unit add` scaffolds a compilable starter and wires it into the monad. Pick a language:

```console
$ monad unit add apps/api --lang go
✓ scaffolded apps/api as 'api' (go)

files:
  apps/api/go.mod
  apps/api/main.go
  apps/api/unit.toml
  profiles/release.toml          # 'api' added to units list

next:
  monad plan
```

The starter is a working "hello world" that compiles, tests, and lints out of the box. Open `apps/api/main.go`, edit it however you want; monad doesn't care what's inside as long as `go build ./...` succeeds.

The generated `apps/api/unit.toml`:

```toml
name = "api"
language = "go"
```

That's all — the Go adapter supplies the default `build`, `check`, `test`, `lint` task recipes (`check` runs `go vet`, the fast type-check). You add a `[tasks.<name>]` block here only when you want to override or add a custom task.

## 3. Add a second unit

Repeat with whatever else you want to ship. A frontend:

```console
$ monad unit add apps/web --lang node-npm
✓ scaffolded apps/web as 'web' (node-npm)

files:
  apps/web/package.json
  apps/web/index.js
  apps/web/unit.toml
  profiles/release.toml          # 'web' added to units list

next:
  monad plan
```

Or a Java service, a Python worker, anything monad knows about. Run `monad unit add --help` or see [configuration.md › `language`](./configuration.md#top-level-fields) for the full set of supported languages.

After two unit-adds your tree looks like:

```
myapp/
├── monad.toml
├── profiles/
│   └── release.toml         # name = "release"; units = ["apps/api", "apps/web"]
├── apps/
│   ├── api/
│   │   ├── go.mod
│   │   ├── main.go
│   │   └── unit.toml
│   └── web/
│       ├── package.json
│       ├── index.js
│       └── unit.toml
```

## 4. Plan and run

Same flow as adopting an existing repo:

```console
$ monad plan
plan: release monad (2 units)

  api  (go)
    build  [cache miss]  4c33edbecac0
    lint   [cache miss]  79c74f4a1267
    test   [cache miss]  97c3171912aa

  web  (node-npm)
    build  [cache miss]  78c4ee8bb5dc
    lint   [cache miss]  a017d2f020f8
    test   [cache miss]  e29544641d7f

summary: 2 units · 6 tasks · 6 miss · 0 hit
```

```console
$ monad ci
monad: release (2 units)

  api  (go)
    build  [built    ]  4c33edbecac0     830ms
    test   [built    ]  97c3171912aa     420ms
    lint   [built    ]  79c74f4a1267     280ms

  web  (node-npm)
    build  [built    ]  78c4ee8bb5dc   2,940ms
    test   [built    ]  e29544641d7f   1,610ms
    lint   [built    ]  a017d2f020f8     880ms

summary: 2 units · 6 tasks · 6 built · 0 cached · 0 failed · 6,960ms
```

Run `monad ci` again — every task hits the cache and returns in milliseconds.

## 5. Customise per-unit

Once you're past hello-world, you'll want real tasks. Edit a `unit.toml`:

```toml
# apps/api/unit.toml
name = "api"
language = "go"

outputs = ["bin/api"]

[tasks.build]
run = "go build -o bin/api ./cmd/api"

[tasks.test]
run = "go test -race ./..."
env = ["DATABASE_URL", "REDIS_URL"]

[tasks.migrate]
run = "go run ./cmd/migrate"
inputs = ["**/*.go", "migrations/**"]
env = ["DATABASE_URL"]
```

What that does:

- `outputs = ["bin/api"]` — monad knows where the built binary lives (used by `monad artifacts` for downstream packaging — see [README › Packaging your build artefacts](../README.md#packaging-your-build-artefacts)).
- `[tasks.build].run` — overrides the adapter's default `go build ./...` with your specific command.
- `[tasks.test].env = ["DATABASE_URL", ...]` — these env var **values** are mixed into the cache key. The names show up in `monad why`; the values are hashed only.
- `[tasks.migrate]` — a brand-new task, not one of the standard build/test/lint trio. Run with `monad build api migrate`.

For the full field list with defaults, see [configuration.md › `<unit>/unit.toml`](./configuration.md#unitunittoml).

## 6. Add cross-unit dependencies

When one unit depends on another:

```toml
# apps/api/unit.toml
depends_on = ["lib-shared"]
```

monad builds `lib-shared` first. Any change to `lib-shared`'s inputs cascades down — `api`'s cache key now depends on `lib-shared`'s content, so a `lib-shared` edit invalidates `api`. The pessimistic cascade catches "library changed, but the binary's source files didn't" cases that simpler tools miss.

If you don't want the cascade for a particular unit (say, a utility CLI that genuinely doesn't care about library changes), opt out:

```toml
# apps/cli/unit.toml
force_independent = true
```

Visualise the graph:

```console
$ monad graph
release
├── api
│   └── lib-shared
└── web
```

Or `monad graph --format dot | dot -Tsvg > graph.svg` for something nicer.

## 7. Multiple profiles

For most projects, one monad is fine. When you want logical groupings (deploy backend before frontend, ship a `core` set then `extras`, separate `oss` and `enterprise` builds), add more:

```toml
# profiles/backend.toml
name = "backend"
units = ["apps/api", "lib-shared"]
```

```toml
# profiles/frontend.toml
name = "frontend"
units = ["apps/web"]
```

```console
$ monad ci --monad backend     # build/test/lint just the backend
$ monad ci --monad frontend    # ... or just the frontend
$ monad ci                     # ... or every monad, deduped
```

A unit in multiple profiles is built once; the content cache is shared. See [configuration.md › Example workspaces](./configuration.md#example-workspaces) for worked layouts.

## 8. Wire into CI

Drop the GitHub Action in:

```yaml
# .github/workflows/ci.yml
name: CI
on: [push, pull_request]

jobs:
  monad:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: thomascarter613/monad-next@v0.1
        with:
          version: '0.1.0'
```

That's the whole file. The action installs monad, restores its content cache, fetches every pinned toolchain into `~/.monad/tools/`, and runs `monad ci`. No `actions/setup-*` chain — monad's adapters fetch the right Go / Node / Java / etc. for you, sourced from the `[toolchain]` pins your units captured.

See [README › Toolchain handling](../README.md#toolchain-handling) for the opt-out path if you'd rather chain `actions/setup-*` yourself.

## What now

- **Agent fix-up loops** — when a `cargo` / `golangci-lint` / `eslint` / `ruff` task fails, the JSON report's `diagnostics` array gives you parsed `{file, line, severity, message, rule}` records ready to feed back to an agent. `monad schema diagnostics` for the shape.
- **Package and deploy** — see [README › Packaging your build artefacts](../README.md#packaging-your-build-artefacts) for two patterns (convention vs reading the `artifacts` action output).
- **Diagnose cache surprises** — `monad why <hash>` returns the full input manifest behind any cache key.
- **Add a third-party language** — write a [plugin](./plugins.md) (~200 lines of pure-`std` Rust per the reference example).
- **Health check** — `monad doctor` periodically catches config drift.

For the deep config reference, see [configuration.md](./configuration.md). For commands and flags, run `monad --help` and `monad <command> --help`.
