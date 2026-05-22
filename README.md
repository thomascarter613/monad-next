<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/logo-dark.svg">
  <img src="docs/assets/logo.svg" alt="monad" width="360">
</picture>

**A polyglot monorepo orchestrator — built for agents, first-class for humans.**

[![CI](https://github.com/thomascarter613/monad-next/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/thomascarter613/monad-next/actions/workflows/ci.yml?query=branch%3Amain)
[![Release](https://github.com/thomascarter613/monad-next/actions/workflows/release.yml/badge.svg)](https://github.com/thomascarter613/monad-next/actions/workflows/release.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](#license)

</div>

Monad plans, builds, tests, and caches across monorepos that mix any combination of ecosystems — Go, Rust, Python, Ruby, PHP, JVM (Maven and Gradle), and the whole TypeScript/JavaScript family (npm, pnpm, yarn, Bun, Deno). It wraps native package managers rather than replacing them, caches by content hash across local, CI, and remote tiers, and exposes every decision as structured JSON.

The result: CI in seconds, not minutes. Agents that can read, reason, and recover without guesswork. Humans that can ship.

---

## Why monad

Modern teams ship polyglot monorepos, and half the commits are landed by agents. Monad is designed for that reality from the ground up:

- **Terse TOML.** A working `monad.toml` fits on a postcard. Defaults that work.
- **Sub-second cold start.** One Rust binary. No JVM, no Node runtime, no daemon.
- **Structured everywhere.** Every reporting command has `--json` with a published schema (`monad schema <target>`). Every failure is a tagged error. Streaming verbs (`monad dev`, `monad serve`, `monad run`) pass through to the underlying process — `--json` is a no-op there because the wrapped tool decides the output shape.
- **Content-addressed cache.** Blake3 over every input — source, lockfiles, env var values, toolchain version, monad version. Hits are bit-exact, misses are explicable.
- **One-line CI integration.** Drop in the GitHub Action; the cache Just Works.

## For agents

Drop [this `CLAUDE.md` / `AGENTS.md` snippet](./docs/agents.md#drop-in-claudemd--agentsmd-snippet) into any monad-managed repo and your coding agent stops rediscovering which package manager each subdir uses — it'll reach for `monad install` / `monad ci` / `monad deploy` and `--json` everywhere. Full rationale + verb table in [docs/agents.md](./docs/agents.md).

Then, the primitives the snippet leans on:

- **`--json` on every command.** Stable, schemaed, parseable.
- **`monad schema <type>`** prints the JSON Schema for every output — `plan`, `report`, `why`, `scaffold`, `doctor`, `manifest`, `error`, `diagnostics`, `notification-payload`, `prime`. Agents can switch on the shape they actually observe.
- **`monad why <hash>`** returns the full input manifest behind any cache key: adapter, toolchain, env var names (never values), and every hashed file's individual blake3 digest. Cache surprises become diagnosable rather than mysterious.
- **Structured errors.** Failures emit `{ kind, message, hint?, where?, docs_url? }` with a stable `kind` taxonomy. No prose-parsing to recover.
- **`monad doctor`** runs a non-destructive sweep — config, toolchains, cache tiers, git, remote — and emits one structured status (`ok | warn | fail | skipped`) per check.
- **`monad unit add <path> --lang <ecosystem>`** scaffolds a compilable starter and wires it into the target monad in one shot, so agents can land green code without toolchain spelunking.
- **`monad init`** in an existing monorepo walks subdirs, auto-detects every unit monad knows about, and captures toolchain pins from each — no hand-wiring.
- **`monad artifacts --json`** emits `{unit: [absolute_paths...]}` so packaging steps (Docker context, upload-artifact, release upload) can find what was built without re-globbing in YAML.
- **Structured tool diagnostics on failure** — when a `cargo` / `eslint` / `golangci-lint` / `ruff` task fails, monad re-runs with the tool's JSON flag and surfaces a `[{file, line, severity, message, rule, source}]` array on the failed task in the report. Agents fix code without parsing tool-specific stderr formats. Schema via `monad schema diagnostics`. Plugin languages can extend the registry via the wire protocol.

## For humans

- **Pretty CLI output.** `monad plan` shows a cache-aware task list; `monad ci` tells you what built, what cached, and what was flaky.
- **`monad serve <monad>`** — hot-reload every unit in a monad with one command.
- **Terse TOML, sensible defaults.** No boilerplate ceremony.
- **One-line GitHub Action** — `uses: thomascarter613/monad-next@v0.1`.

## The 60-second tour

```toml
# monad.toml — optional; every field shown is defaulted
[cache]
local = true
```

```toml
# profiles/release.toml — which units belong to a deployment grouping
name = "release"
units = ["apps/api", "apps/web"]
```

```toml
# apps/api/unit.toml
name = "api"
language = "go"

outputs = ["bin/api"]

[tasks.build]
run = "go build -o bin/api ./cmd/api"

[tasks.test]
run = "go test ./..."
```

```console
$ monad ci
monad: release (2 units)

  api  (go)
    build  [cache hit]   6486e15107b0      30ms
    test   [built    ]   3f21c9a4dd8c   1,820ms

  web  (node-npm)
    build  [cache hit]   b842fe11dce4      28ms
    test   [built    ]   0a56c2917bdd   3,610ms

summary: 2 units · 4 tasks · 2 built · 2 cached · 0 failed · 5,488ms
```

```console
$ monad why 6486e15107b0
key: 6486e15107b082d269515ba7c959106116b9795c3cc9073950c6415466d4abf1
  unit:           api
  task:           build
  command:        go build -o bin/api ./cmd/api
  monad version:  0.1
  adapter:        go
  toolchain:      go:1.22.3
  hashed files (3):
    go.mod        3292399cafff         42 bytes
    go.sum        de0021b00bcc          0 bytes
    main.go       ef4321098765         51 bytes
```

For deeper config: see [docs/configuration.md](./docs/configuration.md).

## Walkthroughs

Two end-to-end guides take you from `monad --version` to green CI in 10 minutes:

- **[Adopting monad in an existing repo](./docs/adopt-existing-repo.md)** — `monad init` walks your monorepo, auto-detects every unit monad knows about, captures toolchain pins, and wires up the config without touching your sources.
- **[Starting a new project with monad](./docs/new-project.md)** — `monad init` plus `monad unit add` scaffolds a compilable polyglot starter from scratch.

## Vocabulary

- **monad** — a deployment grouping; a set of units you want to plan, build, test, or cache as a unit
- **unit** — an app inside a monad (`api`, `web`, a worker, a CLI, a library)
- **task** — an action on a unit (`build`, `check`, `test`, `lint`)

A monad is **whatever logical grouping makes sense to you**. Monad is unopinionated about why you group units; only that you can. Some examples of how teams use them:

- **Logical layers** — `backend` (api + billing + scheduler), `frontend` (web + admin)
- **Release stages** — `core` (must deploy first), `extras` (depends on core)
- **Environments** — `staging`, `prod` (each defining a slightly different unit set)
- **Tiers** — `oss`, `enterprise`
- **Anything else** — `daily` vs `nightly`, `customer-a` vs `customer-b`, ...

A unit can belong to **more than one monad** and its cache is shared across them. The same `api` unit in both `backend` and `release` is the same hashed artefact — built once, reused everywhere.

Single-monad monorepos are common too; one `profiles/all.toml` with every unit is fine.

## Configuration

Three TOML files, by convention:

| File | Purpose | Required? |
|------|---------|-----------|
| `monad.toml` | Repo-wide defaults: cache tiers, toolchain pins, plugin filters | optional (defaults work) |
| `profiles/<name>.toml` | One per monad — names the grouping and lists its units | at least one |
| `<unit>/unit.toml` | One per unit — language, tasks, outputs, dependencies | one per unit |

A minimal workspace needs just one `profiles/<name>.toml` and one `unit.toml`. Defaults in `monad.toml` apply to everything; per-unit `unit.toml` can override toolchain pins, declare task-specific inputs/outputs, opt out of dep cascade, and pin retry behaviour for flaky tests.

**Full reference**: [docs/configuration.md](./docs/configuration.md) — every field in every file with examples and defaults.

## Commands

| Command | What it does |
|---------|--------------|
| `monad init` | Bootstrap a workspace; auto-detect units in subdirs, capture toolchain pins |
| `monad unit add <path>` | Add a unit (scaffold new code or adopt an existing dir) |
| `monad add <pkg>… [--unit <d>] [--dev]` | Add a dependency to a unit via its native package manager (cargo / bun / npm / pnpm / yarn / go) |
| `monad plan` | Show what would build and why; cache hit/miss per task |
| `monad ci` | Plan and execute everything; the GitHub Action entry point |
| `monad build \| check \| test \| lint [target]` | Run one task across a monad or single unit (`check` is the fast type-check verb — `cargo check`, `go vet`) |
| `monad deploy [target]` | Run deploy integrations (Railway, Vercel, Cloudflare Pages, Cloudflare Workers); fires post-deploy notifications |
| `monad notify [target]` | Replay post-deploy notifications (Slack, Linear, …) from the last deploy's cached payload |
| `monad serve <monad>` | Hot-reload every unit in a monad |
| `monad dev <unit>` | Run one unit in dev mode |
| `monad run <unit> <task> -- args…` | Invoke a `[tasks.<task>]` block ad-hoc; bypasses cache. Use for CLIs, migrations, one-off scripts. |
| `monad why <hash>` | Explain a cache key — every hashed input, with digests |
| `monad graph [monad]` | Print the dependency graph (ASCII or DOT) |
| `monad doctor` | Health check: config, toolchains, cache, git, remotes |
| `monad artifacts` | List resolved output paths per unit (post-build) |
| `monad cache stats \| clear \| push \| pull` | Inspect, clear, or sync cache tiers |
| `monad toolchain list \| install \| pin` | Manage pinned language toolchains |
| `monad schema [target]` | Emit JSON Schema for any agent-consumable output type |

Every reporting command takes `--json` for machine-readable output. (Streaming verbs — `monad dev`, `monad serve`, `monad run` — pass through to the wrapped process, so `--json` is a no-op there.) Run `monad <command> --help` for full flag detail.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/thomascarter613/monad-next/v0.1.0/install.sh | sh -s -- 0.1.0
```

For private releases, set `GITHUB_TOKEN` or use `gh release download` manually. The GitHub Action handles both cases transparently.

### Verifying your install

Right after the curl-pipe-sh, walk this sequence — it's the canonical smoke-test:

```sh
monad --version                           # 1. binary on PATH and runnable
monad prime                               # 2. workspace discovery + cache state
monad-mcp --help                          # 3. companion MCP binary also on PATH
monad mcp install                         # 4. register monad-mcp in every detected agent client
                                          #    (Claude Code, Claude Desktop, Cursor, Windsurf,
                                          #     Codex CLI, OpenCode, Zed)
# restart the affected client(s)
# in your client: list MCP tools — you should see mcp__monad__prime, mcp__monad__plan, etc.
monad doctor                              # 5. structured health check (workspace, toolchains, cache)
```

If step 4 reports `no agent clients detected`, install one (or pass the client name explicitly: `monad mcp install claude-code`). If `monad doctor` returns any `fail` rows, the message tells you exactly what's wrong; if you'd like a JSON form for an agent to consume, run `monad doctor --json`.

## GitHub Action

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
          monad: release
```

That's the whole workflow. The action installs monad, restores its content cache, fetches every pinned toolchain from your units' `[toolchain]` blocks (and caches that too), and runs the build. No `actions/setup-*` chaining required for any language monad knows about.

### What that replaces

A typical polyglot monorepo CI workflow without monad — pin each toolchain, set up each runtime, restore each per-tool dependency cache, run each tool, parse each tool's output:

```yaml
# Without monad — every language adds its own setup + cache + invocation
jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - uses: actions/setup-go@v5
        with: { go-version-file: 'apps/api/go.mod' }
      - uses: actions/cache@v4
        with: { path: ~/go/pkg/mod, key: go-${{ hashFiles('apps/api/go.sum') }} }
      - run: cd apps/api && go build ./... && go test ./... && go vet ./...

      - uses: actions/setup-node@v4
        with: { node-version-file: 'apps/web/.nvmrc' }
      - uses: actions/cache@v4
        with: { path: ~/.npm, key: npm-${{ hashFiles('apps/web/package-lock.json') }} }
      - run: cd apps/web && npm ci && npm run build && npm test && npm run lint

      - uses: shivammathur/setup-php@v2
        with: { php-version-file: 'services/billing/.php-version' }
      - uses: actions/cache@v4
        with: { path: services/billing/vendor, key: composer-${{ hashFiles('services/billing/composer.lock') }} }
      - run: cd services/billing && composer install && vendor/bin/phpunit && vendor/bin/phpstan analyse

      - uses: actions/setup-java@v4
        with: { distribution: 'temurin', java-version-file: 'services/scoring/.java-version' }
      - uses: actions/cache@v4
        with: { path: ~/.m2/repository, key: m2-${{ hashFiles('services/scoring/pom.xml') }} }
      - run: cd services/scoring && mvn package -DskipTests && mvn test && mvn verify -DskipTests
```

```yaml
# With monad — same monorepo, same toolchains, same tasks
jobs:
  ci:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: thomascarter613/monad-next@v0.1
        with: { version: '0.1.0' }
```

The monad version doesn't just *look* shorter — it actually does more: every task is content-hashed individually so unrelated changes don't re-run unrelated tasks, every cache the without-monad workflow declared explicitly (`~/.npm`, `~/go/pkg/mod`, `~/.m2/repository`, `~/.cache/composer`, ...) is wrapped automatically, plus you get monad's content cache *on top* (skip the task entirely when nothing changed) and a single structured `report` output instead of N tool-specific log shapes.

| Input | Description |
|-------|-------------|
| `version` | Pinned monad version to download. Much faster than source-build. |
| `monad` | Filter to one monad (equivalent to `--monad` on the CLI). |
| `task` | One of `ci` (default), `build`, `check`, `test`, `lint`, `deploy`, `notify`. `check` runs the adapter-native fast type-check (`cargo check`, `go vet`) — useful as a cheap PR gate. |
| `target` | Monad or unit name for `build` / `check` / `test` / `lint` / `deploy`. |
| `env` | For `task: deploy` — named env profile from `monad.toml [environments.<name>]` (secret aliases + doctor scope). |
| `secret-from` | For `task: deploy` — newline-separated `DECLARED=SOURCE` aliases. Ad-hoc alternative to `env`. |
| `preview` | For `task: deploy` — run preview deploys (`--preview`). Mutually exclusive with `rollback`. |
| `rollback` | For `task: deploy` — roll back to the previous deploy. Mutually exclusive with `preview`. |
| `workspace-path` | Directory containing `monad.toml` and `profiles/` (default: checkout root). |
| `json` | Emit the execution report to stdout / workflow outputs as JSON. |
| `cache-key-suffix` | Bump to force a cold cache after a monad version upgrade. |
| `source-path` | Path to a monad Cargo workspace — used as a fallback when `version` is empty. |
| `install-toolchains` | When `true` (default), action runs `monad toolchain install` and caches `~/.monad/tools/`. Set `false` to chain `actions/setup-*` yourself. |

| Output | Description |
|--------|-------------|
| `report` | Full ExecutionReport JSON — task results, cache hit/miss, durations. Always set. Same shape as `monad ci --json`; canonical schema from `monad schema report`. |
| `artifacts` | JSON object `{unit_name: [absolute_paths...]}` resolved from each unit's `[outputs]` after the run. Pipe to `jq` from a downstream step (Docker, upload-artifact, release). |
| `toolchains-installed` | JSON listing toolchains the action fetched. Set when `install-toolchains: true`. Same shape as `monad toolchain install --json`. |
| `json` | Same content as `report`, set only when `json: true`. Kept for back-compat. |

### Toolchain handling

By default, the action runs `monad toolchain install` after restoring caches and before the build. Monad's embedded mini-mise fetches every **explicitly-pinned** toolchain version into `~/.monad/tools/`, and `actions/cache@v4` wraps that directory keyed on the hash of every TOML + `.tool-versions` in your workspace. Subsequent runs hit the cache and start the build immediately.

**What counts as "explicitly pinned"** — only the `[toolchain]` block in `monad.toml` (repo-wide) or `unit.toml` (per-unit) triggers auto-install:

```toml
# monad.toml — repo-wide defaults
[toolchain]
go = "1.22.3"
node = "22.1.0"
```

```toml
# unit.toml — per-unit override
[toolchain]
node = "20.10.0"
```

Pins that the *adapter* detects from your project files (`.nvmrc`, `.node-version`, `.tool-versions`, `.java-version`, `go.mod`'s `go` directive, `engines.node` in `package.json`, etc.) are folded into the cache key — so a `.nvmrc` bump invalidates — but they do **not** trigger auto-install. That behaviour is deliberate: auto-installing from an adapter-detected file would silently override whatever toolchain the host shell already has, which is usually surprising on a local machine. An explicit `[toolchain]` block is the user opting in.

**Concretely:**

- Your unit has `[toolchain] go = "1.22.3"` → action installs Go 1.22.3 into `~/.monad/tools/` and prepends to PATH for each task. No `setup-go` needed.
- Your unit has `.nvmrc` with `22.1.0` and no `[toolchain]` block → monad uses whatever `node` is on PATH; adds `node=22.1.0` to the cache key so a `.nvmrc` edit invalidates. **Add `setup-node` yourself** if you want Node 22.1.0 on PATH in CI.

### Opting out / mixing with `setup-*`

If you'd rather use the upstream `actions/setup-*` actions (Volta-style version switching, distribution choice for `setup-java`, bespoke configs monad doesn't reproduce), opt out:

```yaml
jobs:
  monad:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: actions/setup-go@v5
        with: { go-version-file: 'apps/api/go.mod' }
      - uses: actions/setup-node@v4
        with: { node-version-file: 'apps/web/.nvmrc' }
      - uses: thomascarter613/monad-next@v0.1
        with:
          version: '0.1.0'
          install-toolchains: 'false'
```

When you set `install-toolchains: false`, you'll typically also want `[toolchain] use_system = true` in `monad.toml` so monad itself doesn't try to install on top of your `setup-*`-provided versions.

Three caches are wired up automatically — no `actions/cache` step of your own needed:

| Cache | What's in it | Key |
|-------|--------------|-----|
| `~/.monad/cache` | Task **results** (skip the task entirely on a hit) | per-`github.sha` rolling forward |
| `~/.monad/tools` | Installed language toolchains | hash of every `monad.toml` / `unit.toml` / `.tool-versions` |
| `~/.npm`, `~/.m2/repository`, `~/.cache/composer`, `~/go/pkg/mod`, `~/.cargo/registry`, `~/.gradle/caches`, etc. | The package managers' **own** download caches — what they pull on a cache miss when monad decides the task needs to actually run | hash of every lockfile across the workspace |

Together they mean the monad workflow is faster than a hand-tuned per-tool CI on **both** cache hits (skip the task) and cache misses (warm dep download cache).

## Deploying with monad

One verb — `monad deploy` — replaces whatever combination of platform CLIs your monorepo would otherwise juggle. Ships with built-in Railway, Vercel, Cloudflare Pages, and Cloudflare Workers integrations; the `Integration` trait extends to anything else (Fly, Netlify, Sentry, Docker registry, …). Every deploy can chain **notifications** (post-deploy webhooks — Slack posts, Linear status flips, PagerDuty triggers) that receive a structured JSON payload on stdin and never break the build when a webhook is down.

```toml
# unit.toml — Railway-deployed backend
[integrations.railway]
service = "backend"
root    = ".."
```

```toml
# monad.toml — named secret profiles (aliases, never values)
[environments.staging]
secrets.RAILWAY_TOKEN = "RAILWAY_TOKEN_STAGING"
```

```console
$ monad deploy --env staging backend
  backend  (go)
    build           [cache hit ]
    railway:deploy  [built     ]   4s
      output: Build Logs: https://railway.com/project/.../deploy/abc123
```

In GitHub Actions, one step wraps preflight (`monad doctor --env <env>`) + the deploy with full JSON report:

```yaml
- uses: thomascarter613/monad-next@v0.1
  with:
    version: '0.1.0'
    task: deploy
    env: ${{ github.event_name == 'release' && 'prod' || 'staging' }}
  env:
    RAILWAY_TOKEN_STAGING: ${{ secrets.RAILWAY_TOKEN_STAGING }}
    RAILWAY_TOKEN_PROD:    ${{ secrets.RAILWAY_TOKEN_PROD }}
```

Full guide — multi-service fan-out, secret alias resolution, preflight diagnostics, staging/prod split patterns, troubleshooting — in [**docs/deploying.md**](./docs/deploying.md).

## Packaging your build artefacts

monad builds artefacts and tells you where they are. For image / registry pushes (`docker/build-push-action`, `aws-actions/*`, `actions/upload-artifact`, …) monad hands off rather than reinventing. Two ways to bridge:

**Pattern 1 — convention (recommended).** Your `unit.toml`'s `outputs` and your `Dockerfile`'s `COPY` paths just agree:

```toml
# apps/api/unit.toml
name = "api"
language = "go"

outputs = ["bin/api"]

[tasks.build]
run = "go build -o bin/api ./cmd/api"
```

```dockerfile
# apps/api/Dockerfile
FROM gcr.io/distroless/base
COPY bin/api /usr/local/bin/api
ENTRYPOINT ["/usr/local/bin/api"]
```

```yaml
# .github/workflows/ci.yml
- uses: thomascarter613/monad-next@v0.1
  with: { task: build, target: api, version: '0.1.0' }
- uses: docker/build-push-action@v5
  with:
    context: apps/api    # monad built bin/api here; Dockerfile COPYs it
    push: true
```

The monad step rebuilds (or cache-hits) `apps/api/bin/api`. The Docker step picks it up via the path the Dockerfile already names. No glue YAML.

**Pattern 2 — read paths from the action output.** When your build context isn't co-located with the unit (e.g. you compose artefacts from several units into one image), use the `artifacts` output:

```yaml
- id: monad
  uses: thomascarter613/monad-next@v0.1
  with: { task: build, version: '0.1.0' }

- name: Stage build context
  run: |
    mkdir -p build-context
    echo '${{ steps.monad.outputs.artifacts }}' \
      | jq -r '.api[], .web[]' \
      | xargs cp -r -t build-context/
    cp Dockerfile build-context/

- uses: docker/build-push-action@v5
  with: { context: build-context, push: true }
```

Locally, `monad artifacts --json` returns the same shape; pipe to `jq` from any shell.

## Caching

Monad's cache is content-addressed (blake3) over every task input — source
files, lockfiles, env var values, adapter + toolchain (declared *and*
resolved), the `monad` version. Three tiers:

- **Local** (default). On disk at `~/.monad/cache`. Nothing to configure.
  Inspect with `monad cache stats`; reset with `monad cache clear`.

- **GitHub Actions.** The composite action wraps `~/.monad/cache` with
  `actions/cache@v4`, scoped per-branch via the standard GHA cache API.
  No extra config needed — use the action and caching happens.

- **Remote.** Two URL schemes, pick whichever suits your ops story.
  Both read-through on local miss, write-back on successful build, and
  never fail the build on network error.

  **S3-compatible (`s3://…`)** — any object store speaking the S3
  protocol: AWS S3, Cloudflare R2, MinIO, Backblaze B2, or your own
  proxy. Credentials from the standard AWS env chain
  (`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN`).

  ```toml
  # monad.toml
  [cache]
  remote = "s3://my-bucket/optional/prefix"
  remote_region = "us-east-1"
  # For non-AWS S3-compatible services (R2, MinIO, Backblaze B2):
  # remote_endpoint = "https://<account>.r2.cloudflarestorage.com"
  ```

  **Hosted Bearer-auth (`monad://…`)** — the same wire protocol as the
  hosted cache at `cache.monad.build`, served by any server that speaks
  it. Credential is a JWT. Interactive devs run `monad login` once and
  the token lands in the OS keychain; CI jobs set the named env var
  directly.

  ```toml
  # monad.toml
  [cache]
  remote = "monad://cache.monad.build"
  remote_token_env = "MONAD_CACHE_TOKEN"
  ```

  Resolution order for reads: `$MONAD_CACHE_TOKEN` → OS keychain
  (`monad` / `cache-token`, written by `monad login`) →
  `~/.monad/credentials` (0600 fallback for headless/keychain-less
  environments). First non-empty wins.

  `monad cache push` / `monad cache pull` do bulk sync between tiers on
  either scheme.

## Plugins

Need a language monad doesn't ship with? Drop a binary named
`monad-adapter-<id>` on `$PATH` that speaks the JSON-RPC-over-stdio
protocol. No fork, no recompile, no dependency on any monad crate —
the [reference noop plugin](./examples/monad-adapter-noop) is ~200
lines of pure-`std` Rust.

Filter discovery via `monad.toml`:

```toml
[plugins]
disable   = ["zig"]                # never load these
allowlist = ["erlang", "elixir"]   # if set, load ONLY these
```

Built-ins always win on id collision. See the
[plugin authoring guide](./docs/plugins.md) for the wire walkthrough,
worked Rust + Python examples, the trust model, and known limits.

## Documentation

Everything in this README plus deeper detail in `docs/`. Humans browse from here; agents discover via `monad --help`, `monad <cmd> --help`, and `monad schema` for structured output formats.

| Doc | What's in it |
|-----|--------------|
| [Walkthrough: adopting an existing repo](./docs/adopt-existing-repo.md) | From cold checkout to green CI on a monorepo monad has never seen. |
| [Walkthrough: starting a new project](./docs/new-project.md) | From `mkdir` to a working polyglot scaffold with one monad and two units. |
| [Configuration reference](./docs/configuration.md) | Every field in `monad.toml` / `profiles/*.toml` / `unit.toml` — including `[environments.<name>]` and `[integrations.<id>]`. |
| [Deploying with monad](./docs/deploying.md) | `monad deploy`, Railway / Vercel / Cloudflare Pages / Cloudflare Workers integrations, multi-service fan-out, secret aliases, staging/prod split, troubleshooting. |
| [Using monad with coding agents](./docs/agents.md) | Drop-in `CLAUDE.md` / `AGENTS.md` snippet, verb reference, when-not-to-use guidance. |
| [Plugin authoring guide](./docs/plugins.md) | Wire protocol walkthrough, Rust + Python reference plugins, trust model. |
| [README → GitHub Action](#github-action) | Inputs, outputs, toolchain contract, packaging patterns. |
| [README → Packaging](#packaging-your-build-artefacts) | Hand off built artefacts to Docker / upload-artifact / release steps. |
| [CHANGELOG](./CHANGELOG.md) | Release-by-release feature log. |

For machine-readable output schemas: `monad schema [target]` (run with no target to list all schemas — `plan`, `report`, `why`, `scaffold`, `doctor`, `manifest`, `error`).

## Status

**Latest release: `v0.1.0` (2026-05-03).** See the [CHANGELOG](./CHANGELOG.md) for every release's notes.

**Platforms.** v0.1 ships prebuilt binaries for Linux (x86_64 + aarch64) and macOS (x86_64 + aarch64). **Windows support is coming in v0.2** — until then, Windows users can `cargo install monad-cli` to build from source (most code paths work; a handful of Unix-isms are tier-2 follow-up work).

Shipping features:

- **13 built-in language adapters** — Go, Cargo, Python (pip + uv), Ruby, PHP, Maven, Gradle, npm, pnpm, yarn, Bun, Deno. Plus a subprocess plugin protocol for anything else.
- **`monad init`** auto-detects every unit in an existing monorepo and captures toolchain pins from the ecosystem's standard files (`.nvmrc`, `go.mod`, `rust-toolchain.toml`, `.tool-versions`, `volta.node`, `engines.node`, …).
- **`monad unit add`** scaffolds new units or adopts existing code with one command.
- **Embedded toolchain manager** — pinned Go / Rust / Node / Python / … versions auto-installed by the action, wired into every task's `PATH`.
- **3-tier content cache** — local CAS, GitHub Actions cache via the composite action, S3-compatible remote (MinIO, R2, any HTTP proxy).
- **Cross-unit dep graph** with pessimistic cascade; `force_independent` foot-gun for escape hatches.
- **`monad dev`** (one unit) and **`monad serve`** (every unit in a monad, with hot reload + prefixed log lines).
- **Opt-in container execution** via `[execution] image = "..."`.
- **Structured tool diagnostics** on failed tasks — cargo / eslint / golangci-lint / ruff today, plugin-extensible.
- **Deploy integrations** (Railway, Vercel, Cloudflare Pages, Cloudflare Workers) with multi-service fan-out, secret-alias resolution via `[environments.<name>]`, preflight gates via `monad doctor`.
- **Notifications** — post-deploy Notify-kind integration tasks (Slack + Linear built in, `[[notifications]]` escape hatch for custom scripts). JSON payload on stdin. Webhook failures never fail the build. `monad notify` replays the last deploy's payload after a fix.
- **`monad artifacts`**, **`monad schema`** (every output has a published JSON Schema), structured errors, `monad doctor`, TTY-aware human output.

## Contributing

Bug reports and PRs welcome; please open an issue first for anything bigger than a small fix so we can discuss the shape before you write code.

**Enable the pre-push gate once per clone** so your push can't get rejected by CI for something local `cargo` would catch:

```sh
git config core.hooksPath .githooks
```

The `.githooks/pre-push` hook runs the same four checks CI does — `cargo fmt --check`, `cargo clippy --locked`, `cargo build --locked`, `cargo test --locked` — only when Rust files are in the push, and short-circuits on doc-only / markdown-only changes. Skipped automatically for deletions and non-Rust pushes. If you need to bypass in an emergency: `git push --no-verify`.

## License

Dual-licensed under either [MIT](./LICENSE-MIT) or [Apache-2.0](./LICENSE-APACHE), at your option.
