# Configuration reference

Monad is configured by three TOML files, by convention:

| File | Purpose | Required? |
|------|---------|-----------|
| `monad.toml` | Repo-wide defaults: cache tiers, toolchain pins, plugin filters, container execution | optional (every field defaulted) |
| `profiles/<name>.toml` | Names a deployment grouping and lists its units | at least one |
| `<unit>/unit.toml` | Names a unit, declares its language and tasks | one per unit |

A minimal workspace needs just one `profiles/<name>.toml` and one `unit.toml`. The repo-wide `monad.toml` is optional and every field has a working default.

This page documents every field. For the conceptual model (profiles vs units vs tasks) see the [README](../README.md#vocabulary).

---

## `monad.toml`

Optional repo-wide defaults. Place at the repo root next to `profiles/`. Every field shown here matches the built-in default — you only need to write the file at all to override something.

```toml
# monad.toml — repo-wide defaults

[defaults]
# Max units to run in parallel within one level of the dep graph.
# Omit to auto-size to std::thread::available_parallelism().
parallelism = 4
# Abort at the next dep-graph level boundary on the first failed unit.
fail_fast = true

[cache]
# Local content-addressed cache at ~/.monad/cache.
local = true
# GitHub Actions cache tier. true | false | "auto" (= on inside a workflow).
gha = "auto"

# Remote cache — pick ONE of the two URL schemes below.
#
# 1. S3-compatible (any bucket: AWS, Cloudflare R2, MinIO, Backblaze B2):
remote = "s3://my-bucket/optional/prefix"
remote_region = "us-east-1"
# remote_endpoint = "https://<account>.r2.cloudflarestorage.com"  # non-AWS only
# Credentials from the AWS env chain:
#   AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY / AWS_SESSION_TOKEN
#
# 2. Hosted monad cache (or any Bearer-auth HTTP server implementing the
#    same wire protocol):
# remote = "monad://cache.monad.build"
# remote_token_env = "MONAD_CACHE_TOKEN"   # env var holding the JWT
# Credential resolution: env var first, then the OS keychain entry
# written by `monad login`, then ~/.monad/credentials (0600) as a
# headless fallback. Run `monad login` once for interactive setup;
# use $MONAD_CACHE_TOKEN in CI.

[telemetry]
# Build reports — sent only to the `monad://` cache remote configured
# above. Wire shape: package, branch, sha, cache_hit_ratio, status,
# duration_ms (no PII, env values, or command lines). Self-hosters with
# no `monad://` remote: nothing is ever sent. Opt out via this flag,
# or per-machine via `MONAD_TELEMETRY=0` (also accepts `false` / `no`
# / `off`). `monad doctor` reports the resolved posture.
enabled = true

[execution]
# Container execution mode. never | auto | always.
#  - never: tasks run on the host (default).
#  - always: every task is wrapped in `<runtime> run --rm ...`.
#  - auto: containerise when an image is declared AND a runtime is on PATH.
container = "never"
# Container image ref to wrap tasks in. Required for container = "always".
image = "ghcr.io/your-org/runner:1"

[toolchain]
# Repo-wide tool version pins. Each `<tool> = "<version>"` writes the
# version into monad's content-cache key, so a toolchain bump invalidates
# every unit that uses it. Per-unit pins (in unit.toml) override these.
go = "1.22.3"
node = "22.1.0"
java = "21"
# When true, monad doesn't try to install pinned versions itself —
# it expects the system PATH to already have the right tool.
use_system = false

[plugins]
# Adapter ids that should never be loaded even if found on $PATH.
disable = ["zig"]
# If set, ONLY these adapter ids are loaded; everything else is skipped silently.
allowlist = ["erlang", "elixir"]
```

### `[defaults]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `parallelism` | int | `available_parallelism()` | Max concurrent units per dep-graph level. |
| `fail_fast` | bool | `true` | Stop at the next dep-graph level boundary on first failure. |

### `[cache]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `local` | bool | `true` | Use the local content-addressed cache at `~/.monad/cache`. |
| `gha` | bool \| `"auto"` | `"auto"` | Use the GitHub Actions cache tier (the composite action wraps `~/.monad/cache` with `actions/cache@v4`). `"auto"` activates only when running inside a GHA workflow. |
| `remote` | string | unset | Remote cache URL. Two schemes: `s3://<bucket>/<optional/prefix>` (any AWS-signed object store), or `monad://<host>[/<prefix>]` (JWT-auth'd HTTP cache — `monad://cache.monad.build` for the hosted service). See README's "Caching" section. |
| `remote_region` | string | `"us-east-1"` | AWS region for the bucket. S3 scheme only. |
| `remote_endpoint` | string | unset | Custom S3-compatible endpoint URL. Required for non-AWS services (Cloudflare R2, MinIO, Backblaze B2); omit for native AWS S3. S3 scheme only. |
| `remote_token_env` | string | `"MONAD_CACHE_TOKEN"` | Name of the env var holding the JWT, for the `monad://` scheme. Resolver walks env var → OS keychain entry `("monad", "cache-token")` (populated by `monad login`) → `~/.monad/credentials` (0600 fallback). Monad never stores the token in repo state. |

### `[telemetry]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Send a build report to the configured `monad://` cache remote after `monad ci` and `monad build`. Wire shape: `package`, `branch`, `sha`, `cache_hit_ratio`, `status`, `duration_ms` — no PII, env values, or command lines. Best-effort POST: failures are logged at `warn`, never block the build. With no `monad://` remote configured, nothing is ever sent regardless of this flag. |

**Opt-out paths** (precedence: either says off → off; the env var cannot force telemetry on if the config disables it):

- `[telemetry] enabled = false` in `monad.toml` — committed, team-wide.
- `MONAD_TELEMETRY=0` (or `false` / `no` / `off`) — per-machine override, useful in CI or local shells.

`monad doctor` reports the resolved posture as the `telemetry.posture` check (`enabled` / `disabled by config` / `disabled by env` / `disabled by both`).

### `[execution]`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `container` | `"never"` \| `"auto"` \| `"always"` | `"never"` | Container execution mode. |
| `image` | string | unset | Container image ref. Required for `container = "always"`; advisory for `"auto"`. |

When containerised, monad runs each task as `<runtime> run --rm -u <uid>:<gid> -v <unit>:/work -w /work --env HOME=/work --env <name> <image> sh -c <run>`. Runtime auto-detection order: `docker` → `podman` → `nerdctl`. UID is preserved so output files stay host-owned.

**Default `HOME=/work`:** the container's `$HOME` defaults to the mounted workdir. `--user <host-uid>` leaves the image's root `$HOME` (often `/root`) unwritable by the invoking UID, so without this default, tools that default their cache dir to `$HOME/.cache/<tool>` — Go (`GOCACHE`), Cargo (`CARGO_HOME`), pnpm, npm — would fail on first run with a permission error. Pointing HOME at the volume mount puts those caches under the unit's writable scratch space and keeps them across invocations. If you genuinely need a different HOME, declare it in `[tasks.<name>] env = ["HOME"]`: the forwarded host value wins (docker `--env` applies last-write per variable).

### `[toolchain]`

A free-form table of `<tool> = "<version>"` pairs, plus the boolean `use_system`. The keys aren't enumerated — monad accepts any `<tool>` name and includes `<tool>:<version>` in the content-cache key.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `use_system` | bool | `false` | If `true`, monad expects the pinned tools to already be on `$PATH` and won't try to install them itself. |
| `<tool>` | string | unset | Pin a tool to a specific version. Examples: `go = "1.22.3"`, `node = "22.1.0"`, `python = "3.12"`, `ruby = "3.2.2"`, `java = "21"`. |

Per-unit `unit.toml` `[toolchain]` overrides these.

### `[plugins]`

Filters applied to subprocess plugin discovery (binaries on `$PATH` matching `monad-adapter-<id>`). See [plugins.md](./plugins.md) for the full plugin protocol.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `disable` | `string[]` | `[]` | Adapter ids to never load. |
| `allowlist` | `string[]` \| unset | unset | If set, ONLY these adapter ids are loaded. |

Built-in adapters always win on id collision regardless of `[plugins]` settings.

### `[environments.<name>]`

Named deploy environments with saved **secret aliases** for `monad deploy --env <name>` and `monad doctor --env <name>`. Each entry maps a **declared** env-var name (what integrations look for, e.g. `RAILWAY_TOKEN`) to a **source** env-var name (what the host shell / CI secret layer exports, e.g. `RAILWAY_TOKEN_STAGING`). Never holds secret *values* — only name-to-name aliases.

```toml
[environments.staging]
secrets.RAILWAY_TOKEN = "RAILWAY_TOKEN_STAGING"
secrets.VERCEL_TOKEN  = "VERCEL_TOKEN_STAGING"

[environments.prod]
secrets.RAILWAY_TOKEN = "RAILWAY_TOKEN_PROD"
secrets.VERCEL_TOKEN  = "VERCEL_TOKEN_PROD"
```

With that block in place, `monad deploy --env staging` reads `$RAILWAY_TOKEN_STAGING` from the host env and exposes it to the deploy task under the name `RAILWAY_TOKEN` (which is what the Railway integration declares as its required env). The same mapping works identically local and in CI — in a GHA workflow you set `env: RAILWAY_TOKEN_STAGING: ${{ secrets.X }}` at the step level and monad resolves through the alias.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `secrets.<DECLARED>` | string | — | Source env-var name whose value should be exposed to tasks under the declared name. Declared must match what an integration / task's `required_env` declares. |

Ad-hoc alternative: `monad deploy --secret-from DECLARED=SOURCE` on the CLI. See [deploying.md](./deploying.md) for the full workflow.

---

## `profiles/<name>.toml`

One file per monad. The file's basename **is** the monad's name in CLI references (e.g. `profiles/release.toml` → `monad ci --monad release`). The `name` field inside the file must match the basename.

A monad is whatever logical grouping makes sense to you — environment, release stage, logical layer, customer tier. Monad is unopinionated about why; only that the units listed here ship together.

```toml
# profiles/release.toml — every unit in this monad ships as a unit
name = "release"
units = [
  "apps/api",
  "apps/web",
  "services/billing",
]
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Monad name. Must match the file's basename and be unique across `profiles/`. No `/` or platform path separators allowed. |
| `units` | `string[]` | yes | List of unit directory paths, relative to the workspace root, in any order. Forward-slashes regardless of host OS. Empty `[]` is valid (a freshly initialised workspace). |

### Multiple profiles

A repo can (and often does) have several profiles:

```
profiles/
├── backend.toml        # api + billing + scheduler
├── frontend.toml       # web + admin
└── release.toml        # everything that goes out together
```

A unit can appear in **multiple profiles**. Its content-cache key is derived from the unit, not the monad, so the same `api` unit in both `backend` and `release` is built once and reused.

---

## `<unit>/unit.toml`

One file per unit, in the unit's directory (which is also the working directory for its tasks). The `name` field is the unit's CLI handle (`monad build api`).

```toml
# apps/api/unit.toml
name = "api"
language = "go"

# Glob patterns mixed into the cache key for *custom* tasks only
# (e.g. `migrate`, `seed`). Lifecycle tasks (build/test/lint, plus
# check on cargo + go) ignore this field — they use the adapter's
# defaults. For those, restate the glob under each [tasks.<name>]
# block. Adapters add their own fingerprint files automatically
# (lockfiles, toolchain pin files, .tool-versions, ...).
inputs = []

# Build artefacts. Globs allowed. Used by `monad artifacts` and by the
# GHA action's `artifacts` output.
outputs = ["bin/api"]

# Other units this one depends on. Monad builds dependencies first,
# and any change to their content invalidates this unit's cache (the
# pessimistic cascade — opt out with force_independent below).
depends_on = ["lib-shared"]

# Skip the dep-cascade for this unit — its cache key is computed from
# its own inputs only. Useful for utility units that genuinely don't
# care about upstream changes.
force_independent = false

# Per-unit toolchain pin. Overrides monad.toml's [toolchain] for this
# unit only.
[toolchain]
go = "1.22.5"

# Tasks. Adapters supply default `build`, `test`, `lint` recipes per
# language (plus `check` for cargo + go — the fast type-check verb);
# declare a [tasks.<name>] block here to override or add.
[tasks.build]
run = "go build -o bin/api ./cmd/api"
# Replaces the adapter default verbatim — restate every glob you want
# in this task's cache key, including any extras (e.g. openapi.yaml).
inputs = ["**/*.go", "go.mod", "go.sum", "openapi.yaml"]
outputs = ["bin/api"]

[tasks.test]
run = "go test ./..."
inputs = ["**/*.go", "go.mod", "go.sum", "openapi.yaml", "testdata/**"]
env = ["DATABASE_URL", "REDIS_URL"]
retry = 1                              # 1 retry → up to 2 attempts

[tasks.lint]
run = "golangci-lint run"

# Optional: hot-reload command for `monad serve` / `monad dev`.
[serve]
run = "air"
```

### Top-level fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | required | Unit handle. Used by CLI flags (`monad build <name>`) and profiles' `units` list. Must be unique across the workspace. |
| `language` | string | adapter-detected | Adapter id (`go`, `cargo`, `python`, `python-uv`, `ruby`, `php`, `maven`, `gradle`, `node-npm`, `node-pnpm`, `node-yarn`, `bun`, `deno`, or any plugin's id). When omitted, monad auto-detects from the unit dir. |
| `package_manager` | string | unset | Reserved for future use; no behaviour today. |
| `inputs` | `string[]` | `[]` | Glob patterns relative to the unit dir, mixed into the cache key only for **custom tasks** (task names outside the adapter's lifecycle set — `build` / `test` / `lint`, plus `check` on cargo + go). Lifecycle tasks use the adapter's default `inputs` and **silently ignore** anything declared here; for those, declare `inputs` under `[tasks.<name>]` instead (see below). Adapters add their own fingerprint files automatically (lockfiles, toolchain pin files, `.tool-versions`). Monad emits a `tracing::warn!` at plan time when a non-empty unit-level `inputs` is shadowed. |
| `outputs` | `string[]` | `[]` | Glob patterns of build artefacts. Listed by `monad artifacts` and the GHA `artifacts` output. |
| `depends_on` | `string[]` | `[]` | Other unit names this unit depends on. Builds upstream first. Changes upstream invalidate this unit (unless `force_independent`). |
| `force_independent` | bool | `false` | Opt out of the pessimistic cascade — only this unit's own inputs go into its cache key. |

### `[toolchain]`

Same shape as `monad.toml`'s `[toolchain]` table — `<tool> = "<version>"` pairs plus optional `use_system`. Per-unit pins override the repo-wide ones.

### `[tasks.<name>]`

Tasks named `build`, `test`, `lint` (plus `check` on cargo + go) get **default recipes from the adapter** for the unit's language. You only need a `[tasks.<name>]` block to:
- Override the default command (e.g. add flags)
- Declare a custom task name (e.g. `migrate`, `seed`, `deploy-preview`)
- Add task-specific `inputs` / `outputs` / `env` / `retry` config

Custom-named tasks (anything outside the adapter's lifecycle set) don't get pulled into `monad ci` — they only run when explicitly invoked via `monad run <unit> <task> -- <args>`. That's the escape hatch for ad-hoc CLIs, migrations, and one-off scripts: same unit-dir cwd + toolchain semantics as a cached task, but the run bypasses the content-hash cache so non-deterministic invocations stay correct.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `run` | string | required | Shell command. Runs from the unit dir, with the unit's `[toolchain]` honoured. |
| `inputs` | `string[]` | adapter default | Glob patterns mixed into the cache key for **this task only**. **Replaces** (does not merge with) the adapter's default `inputs` — restate the adapter globs you still want (e.g. `src/**`, lockfile) plus your additions. Omit to use the adapter's default verbatim. The unit-level `inputs` field is not folded in here either; if you want a glob in every lifecycle task, repeat it under each `[tasks.<name>]`. |
| `outputs` | `string[]` | none | Glob patterns of artefacts produced by this task. Combined with the unit's `outputs` for `monad artifacts`. |
| `env` | `string[]` | `[]` | Names of env vars whose **values** should mix into the cache key. The names are visible (in `monad why`); the values are hashed only. |
| `retry` | int | `0` | Additional attempts on failure. `retry = 2` → up to 3 attempts. A task that succeeds on attempt > 1 is reported `flaky: true` in the execution report. |

### `[serve]`

Optional. Declares the long-running command for `monad serve <monad>` (every unit in a monad) and `monad dev <unit>` (one unit).

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `run` | string | required | Long-running command. Monad spawns it, watches the unit's inputs, and restarts on change. |

### `[integrations.<id>]`

Per-unit config for **integrations** — the second extension point alongside language adapters. Each integration interprets its own block; unknown keys are ignored at load time so fields can be added without monad-config changes. See [deploying.md](./deploying.md) for the full deploy workflow.

#### Railway (`[integrations.railway]`)

```toml
[integrations.railway]
service = "backend"                         # one Railway service to deploy to
# services = ["frontend", "landing-page"]   # OR a list — one deploy task per entry
root = ".."                                 # cd here before `railway up` (monorepo root)
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `service` | string | unset | Railway service name. Injected as `--service <name>` in `railway up`. |
| `services` | `string[]` | unset | Fan out to multiple Railway services that share the same source (e.g. frontend + landing-page with different VITE env vars). One deploy task per entry, named `railway:deploy:<slug>`. Mutually exclusive with `service` (plural wins when both are set). |
| `root` | string | unset | Path (relative to the unit dir) to `cd` to before running `railway up`. Required when your Railway service has `rootDirectory` configured dashboard-side — it needs the full monorepo uploaded. Typically `".."` for top-level units. |

Railway service identity is dashboard-side — Railway's own `railway.json` schema has no `name` / `service` / `slug` field (verified against their schema JSON), so monad owns this mapping.

#### Vercel (`[integrations.vercel]`)

Currently read-only — the Vercel integration emits `vercel:deploy` + `vercel:preview` tasks without per-unit config. Future fields (`team`, `project`, `scope`) will land here.

#### Cloudflare Pages (`[integrations.cloudflare_pages]`)

Config-only opt-in — Pages projects rarely ship a `wrangler.toml` at the unit root (project settings live in the Cloudflare dashboard), so the integration only fires when the block is present.

```toml
[integrations.cloudflare_pages]
project = "my-pages-project"   # required — the CF Pages project name
dist    = "dist"               # default "dist" — the build output dir to upload
branch  = "main"               # default "main" — branch label for the deploy
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `project` | string | required | Cloudflare Pages project name (the slug shown in the dashboard URL: `dash.cloudflare.com/<account>/pages/view/<project>`). Required for both deploys and `monad secret put\|list\|delete`. |
| `dist` | string | `"dist"` | Build output directory (relative to the unit dir) that Wrangler uploads. |
| `branch` | string | `"main"` | Branch label attached to the deploy in the Pages dashboard. |

Wrangler is invoked as `wrangler pages deploy <dist> --project-name <project> --branch <branch> --commit-dirty=true`. `--commit-dirty=true` is always on — monad rebuilds artefacts fresh per invocation, so Wrangler's default git-state check is just noise on monorepos.

#### Cloudflare Workers (`[integrations.cloudflare_worker]`)

Detected via `wrangler.toml` or `wrangler.jsonc` at the unit root. Per-unit config is optional — the default environment in `wrangler.toml` covers the common case.

```toml
[integrations.cloudflare_worker]
env = "production"   # optional — adds --env production to wrangler deploy
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `env` | string | unset | Wrangler environment name. Maps to `[env.<name>]` blocks in your `wrangler.toml`; flows through to `wrangler deploy --env <name>` and to `wrangler secret put\|list\|delete --env <name>`. Omit for the default environment. |

The integration ID is `cloudflare_worker` (singular, code style); the product brand is "Cloudflare Workers" (plural). Same convention as `[dependencies.foo]` vs "the foo crate" elsewhere.

#### Slack (`[integrations.slack]`) — notification

Opt-in Notify-kind integration. Fires after every Deploy task in the unit; posts a templated message to a Slack Incoming Webhook.

```toml
[integrations.slack]
webhook_url_env = "SLACK_WEBHOOK_URL"   # env var holding the https://hooks.slack.com/... URL
channel         = "#deploys"             # optional
username        = "Monad"                # optional
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `webhook_url_env` | string | `"SLACK_WEBHOOK_URL"` | Host env-var name holding the webhook URL. Flows through `[environments.<name>] secrets.*` aliases. |
| `channel` | string | unset | Optional channel override (Slack webhooks pin one at creation; this only takes effect for unpinned webhooks). |
| `username` | string | unset | Optional sender display name. |

#### Linear (`[integrations.linear]`) — notification

Opt-in Notify-kind integration. On a successful deploy, scans the payload for `[A-Z]{2,}-\d+` issue identifiers and transitions each to a target workflow state via Linear's GraphQL API.

```toml
[integrations.linear]
api_key_env       = "LINEAR_API_KEY"
target_state      = "Deployed"
fallback_issue_id = "ENG-1234"   # optional: comment here if no refs matched
team              = "ENG"        # optional: disambiguate state lookup across teams
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `api_key_env` | string | `"LINEAR_API_KEY"` | Host env-var name holding the Personal API key. |
| `target_state` | string | `"Deployed"` | Workflow-state name to transition matched issues to on a successful deploy. |
| `fallback_issue_id` | string | unset | Fallback issue to comment on when no issue refs were discovered. Skipped if unset. |
| `team` | string | unset | Team key (e.g. `"ENG"`). Required only when `target_state` is ambiguous across teams. |

Failed deploys skip transitions entirely — only `fallback_issue_id` comments fire, so a broken release is never marked shipped.

#### Anything else

Plugin integrations read whatever keys they recognise from their own `[integrations.<id>]` block. For custom post-deploy hooks without writing a full `Integration` implementation, use the `[[notifications]]` block below.

### `[[notifications]]`

Custom-script Notify-kind tasks declared inline — escape hatch for bespoke post-deploy hooks where writing a full `Integration` is overkill. Each entry becomes a Notify task that fans out after every Deploy in the unit; the script receives the NotificationPayload JSON on stdin (`monad schema notification-payload`).

```toml
[[notifications]]
name         = "github-pr-comment"
run          = "./scripts/notify-github.sh"
env          = ["GITHUB_TOKEN"]
required_env = ["GITHUB_TOKEN"]
required_cli = ["gh: https://cli.github.com"]
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | yes | Task name in the ExecutionReport. Must be unique within the unit. User-declared `[tasks.<name>]` can override the `run` while keeping Notify semantics intact. |
| `run` | string | yes | Shell command invoked once per Deploy trigger with the NotificationPayload on stdin. |
| `env` | `string[]` | no | Env-var allowlist forwarded to the child (same shape as `[tasks.<name>] env`). |
| `required_env` | `string[]` | no | Env vars that must be set at runtime — preflight fails the notification with a clear message otherwise. |
| `required_cli` | `string[]` | no | CLI binaries that must be on PATH. Entry form: `"binary"` or `"binary: install hint"`. |

Failures never fail the build — same rule as built-in notifications (`summary.notify_failures` tracks them; exit code stays 0).

---

## File resolution and overrides

CLI flags > per-unit `unit.toml` > repo-wide `monad.toml` > built-in defaults.

For toolchains specifically, monad walks each adapter's detection chain to discover an *implicit* version pin (e.g. `go.mod`'s `go 1.22` directive, `.nvmrc`, `.tool-versions`, etc.). That implicit pin counts as the bottom of the override stack — `unit.toml` > implicit detection > nothing.

The fully resolved cache-key inputs for any one task are visible via `monad why <hash>`.

---

## Example workspaces

### Single monad, single unit

The simplest valid workspace. No `monad.toml`.

```
my-app/
├── profiles/
│   └── all.toml             #  name = "all"   units = ["."]
└── unit.toml                #  name = "my-app"  language = "go"
```

### Logical layers, unit reuse

A unit (`shared`) that belongs to multiple profiles.

```
monorepo/
├── monad.toml               # repo-wide cache + toolchain config
├── profiles/
│   ├── backend.toml         # ["services/api", "services/billing", "lib/shared"]
│   ├── frontend.toml        # ["apps/web", "lib/shared"]
│   └── release.toml         # all of the above, in one monad
├── apps/
│   └── web/unit.toml
├── services/
│   ├── api/unit.toml
│   └── billing/unit.toml
└── lib/
    └── shared/unit.toml
```

`shared` is built once when `monad ci --monad release` runs; its cache key is identical no matter which monad you ask for it via.

### Release stages with dependency cascade

Two profiles modelling a deployment ordering: `core` ships first, `extras` depends on `core`. The dep cascade is enforced by `unit.toml`'s `depends_on`, not by the monad boundaries.

```
project/
├── profiles/
│   ├── core.toml            # ["services/auth", "services/users"]
│   └── extras.toml          # ["services/notifications", "services/billing"]
└── services/
    ├── auth/unit.toml       # depends_on = []
    ├── users/unit.toml      # depends_on = ["auth"]
    ├── notifications/unit.toml  # depends_on = ["users", "auth"]
    └── billing/unit.toml    # depends_on = ["users"]
```

Run `monad ci --monad extras` and monad builds `auth` and `users` first (they're upstream), then `notifications` and `billing` in parallel.
