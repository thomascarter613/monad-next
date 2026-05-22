# Deploying with monad

Monad ships **deploy integrations** as a first-class concept alongside language adapters. One verb — `monad deploy` — replaces the "curl-to-platform-CLI" step in whatever per-service shell script currently handles shipping to Railway / Vercel (and anything else via custom `Integration` impls or subprocess plugins). All the same machinery that makes `monad ci` fast and agent-readable — content-addressed cache, structured JSON output, preflight diagnostics, retry + flakiness detection — applies to deploys too.

This page covers the full flow: the mental model, the config surface, the CLI + action shapes, secret handling, and the CI wiring for staging/prod splits.

---

## Mental model

Three layers collaborate on every deploy:

| Layer | What it decides | Where it lives |
|-------|-----------------|----------------|
| **Adapter** | How to build the unit (Go / Node / PHP / ...) | Per-unit `unit.toml` `language = "..."` + the adapter's baked-in recipe |
| **Integration** | How to ship the built artifact (Railway / Vercel / Cloudflare Pages / Cloudflare Workers) | Per-unit `[integrations.<id>]` block + the integration's hardcoded task shape |
| **Environment** | Which secrets to use at deploy time (staging vs prod) | Repo `[environments.<name>]` block — name-to-name alias map only, never secret values |

A unit is claimed by **one** adapter (auto-detected or pinned via `language = "..."`) and **zero-or-more** integrations (every integration whose `detect()` fires on the unit dir). Each integration contributes one or more Deploy-kind tasks that `monad deploy` picks up.

Running `monad deploy`:

1. **Resolve** integrations per unit.
2. **Filter** to units that have a matching integration task. Unites without one get a clear `<no-deploy>` marker in the report (distinguishing "nothing to deploy" from "deploy failed").
3. **Preflight** via `monad doctor --env <name>` — required env vars present, required CLI binaries on `PATH`. Fail fast with a structured error, not a mid-upload 401.
4. **Build** the `build` task first (the canonical prerequisite).
5. **Deploy** by running each integration task (e.g. `railway:deploy`), with declared env vars aliased through the `--env` profile.
6. **Report** as JSON (`monad deploy --json` / `--report-file <path>`) — every task's command, duration, cache key, exit code, and **`output_excerpt`** (the task's stdout — for deploys, that's where the platform's build-log URL lives).

---

## Built-in integrations

### Railway

- **Detects on**: `railway.toml`, `railway.json`, or `.railway/` at the unit root.
- **Emits**: `railway:deploy` (prod) — or `railway:deploy:<slug>` per entry when `services = [...]` fans out.
- **Required env**: `RAILWAY_TOKEN` — typically a project-scoped token generated at Dashboard → Project → Settings → Tokens. Not an account token. See below for CI vs local handling.
- **Required CLI**: `railway`. Installer hint surfaced on failure: `npm install -g @railway/cli` or `brew install railway`.
- **Blocks until the deploy reaches terminal status.** The integration invokes `railway up --ci` — explicit CI mode, which streams build + deploy logs and exits non-zero on `FAILED` / `CRASHED`. Plain `railway up` looks correct but relies on TTY detection; monad runs tasks via `sh -c` with piped stdio, so the no-TTY path silently collapses to a detach-like behaviour where the CLI exits on tarball upload (well before Railway's server-side build) and reports broken releases as successful. `--ci` is the only form that works in both interactive and CI contexts.
- **What "success" actually means.** The `railway` CLI subscribes to Railway's GraphQL deployment-status stream and exits on the *first* status change it observes: `SUCCESS` → exit 0, `FAILED` / `CRASHED` → exit 1. Whether `SUCCESS` waits for your app's healthcheck to pass depends on your Railway-side config: with a healthcheck path set on the service, `SUCCESS` only fires after the check passes; without one, `SUCCESS` fires as soon as the container is running. If you care about crash-on-startup protection, **configure a Railway healthcheck** — otherwise a service that exits 1 five seconds after the process starts may see `SUCCESS` → `CRASHED` and `monad deploy` can exit 0 on the `SUCCESS` it observed first. The fix is Railway-side, not monad-side.

**Per-unit config** (see [configuration.md](./configuration.md#integrationsid)):

```toml
# unit.toml
[integrations.railway]
service = "backend"
root    = ".."   # for monorepos with rootDirectory configured dashboard-side
```

Multiple services sharing one source (e.g. a React app deployed both as `frontend` and `landing-page` with different `VITE_*` env vars Railway sets dashboard-side):

```toml
[integrations.railway]
services = ["frontend", "landing-page"]
root     = ".."
```

…emits two tasks (`railway:deploy:frontend`, `railway:deploy:landing-page`), both Deploy-kind, each with its own cache key and exit status.

**Why `root = ".."`:** Railway services configured with `rootDirectory = "/<subdir>"` expect the full monorepo uploaded so Railway can find their scoped path. `railway up <path>` errors with "prefix not found" when the path is a parent/sibling of cwd (the CLI expects a subpath), so monad `cd`s to the configured root first and runs `railway up` with no path argument.

### Vercel

- **Detects on**: `vercel.json` or `.vercel/project.json`.
- **Emits**: `vercel:deploy` (prod), `vercel:preview` (staging).
- **Required env**: `VERCEL_TOKEN`.
- **Required CLI**: `vercel`. Installer hint: `npm install -g vercel` (or see [vercel.com/docs/cli](https://vercel.com/docs/cli)).

No per-unit config fields today; future `[integrations.vercel] team = "..." project = "..." scope = "..."` will slot in without a monad update.

### Cloudflare Pages

- **Detects on**: nothing — Pages projects opt in via an explicit `[integrations.cloudflare_pages]` block in `unit.toml`. Project settings live in the Cloudflare dashboard, not on disk.
- **Emits**: `cloudflare_pages:deploy`.
- **Required env**: none for `wrangler login`'s OAuth path. CI flows set `CLOUDFLARE_API_TOKEN` (and optionally `CLOUDFLARE_ACCOUNT_ID`); both are forwarded to the deploy task if present.
- **Required CLI**: `wrangler`. Installer hint: `npm install -g wrangler` (or `bun add -g wrangler`).

**Per-unit config** (see [configuration.md](./configuration.md#integrationsid)):

```toml
# unit.toml
[integrations.cloudflare_pages]
project = "my-pages-project"
dist    = "dist"
branch  = "main"
```

The integration runs `wrangler pages deploy <dist> --project-name <project> --branch <branch> --commit-dirty=true`. Wrangler streams build progress and exits non-zero on a failed deploy — no `--ci` flag quirks like Railway. `--commit-dirty=true` is always on because monad rebuilds artefacts fresh per invocation; Wrangler's default git-state check is noise in that flow.

Secrets: `monad secret put <unit> NAME` (and `list` / `delete`) shells out to `wrangler pages secret <op> --project-name <project>`. Reads the value from stdin so it never lands in `ps` / shell history.

### Cloudflare Workers

- **Detects on**: `wrangler.toml` or `wrangler.jsonc` at the unit root.
- **Emits**: `cloudflare_worker:deploy`.
- **Required env**: same as Pages — none for OAuth-logged-in dev, `CLOUDFLARE_API_TOKEN` forwarded if set.
- **Required CLI**: `wrangler`.

**Per-unit config**:

```toml
# unit.toml
[integrations.cloudflare_worker]
env = "production"   # optional — maps to [env.production] in wrangler.toml
```

Translates to `wrangler deploy [--env <env>]`. Wrangler's deploy command is idempotent and blocks on the edge's terminal status — no TTY-conditional behaviour like Railway. The same `env` knob also flows into `monad secret put|list|delete` so multi-environment Workers point their secrets at the matching `[env.<name>]` block.

Note the singular `cloudflare_worker` integration ID vs the brand name "Cloudflare Workers" — code-style identifier, brand-style prose.

---

## Secret aliases

Integrations declare **one canonical env-var name** (`RAILWAY_TOKEN`, `VERCEL_TOKEN`) in their `required_env()`. Users/agents control which **host** env var actually supplies the value at invocation time. Two surfaces, same primitive:

### `[environments.<name>]` in `monad.toml`

Saved named profiles — the human-friendly path:

```toml
# monad.toml
[environments.staging]
secrets.RAILWAY_TOKEN = "RAILWAY_TOKEN_STAGING"
secrets.VERCEL_TOKEN  = "VERCEL_TOKEN_STAGING"

[environments.prod]
secrets.RAILWAY_TOKEN = "RAILWAY_TOKEN_PROD"
secrets.VERCEL_TOKEN  = "VERCEL_TOKEN_PROD"
```

Then:

```sh
monad deploy --env staging               # reads $RAILWAY_TOKEN_STAGING, exposes as $RAILWAY_TOKEN
monad deploy --env prod                  # reads $RAILWAY_TOKEN_PROD
monad doctor --env staging               # preflight honours the aliases
```

An unknown `--env <name>` errors with the list of known profiles — no silent fallback.

### `--secret-from DECLARED=SOURCE` (ad-hoc)

Repeatable, overrides anything from `--env`:

```sh
monad deploy --secret-from RAILWAY_TOKEN=RAILWAY_TOKEN_STAGING \
             --secret-from VERCEL_TOKEN=VERCEL_TOKEN_STAGING
```

The parser **rejects literal values** with a clear hint — passing `--secret-from RAILWAY_TOKEN=rlw_sk_abc123` fails at flag parse with `"source 'rlw_sk_abc123' doesn't look like an env-var name … did you pass the secret value instead of a var name?"`. Secret values on a CLI are an anti-pattern (they leak via `ps`, shell history, `/proc/*/cmdline`); monad only accepts name-to-name indirection.

### How it works under container execution

When a task runs inside a container (`[execution] container = "auto"|"always"`), the container runtime (docker / podman / nerdctl) receives env values via `Command::env(NAME, VALUE) + --env NAME` — the `NAME=VALUE` form on the cmdline is deliberately avoided to keep secret values out of process listings on the host.

### Failure messages surface *both* names

When an aliased env var isn't set, the doctor / executor error says exactly which host var monad looked at:

```
integration.railway.env [fail]  missing env var(s): RAILWAY_TOKEN (via $RAILWAY_TOKEN_STAGING) (units: api)
```

---

## Notifications (post-deploy hooks)

A **notification** is a Notify-kind integration task that fires automatically after every Deploy-kind task in the same unit completes. Think Slack post with the build-log URL, Linear status flip to "Deployed", PagerDuty trigger on deploy failure, GitHub PR comment with a preview URL. All of them are reactive: "the deploy happened, now tell someone."

The rules:

- Every unit's Notify-kind tasks fire once per completed Deploy task in that unit — so two deploys in one unit with one notify integration = two notify invocations, each with its own payload.
- Notify invocations fan out in parallel (they're independent sinks), after the deploy phase is fully done.
- **Failures never fail the build.** A down webhook increments `summary.notify_failures` and logs a warning; exit code stays 0. This matters because a flaky Slack shouldn't red-X a successful prod deploy.
- `monad ci` never runs Notify tasks. They only fire via explicit `monad deploy` (auto) or `monad notify` (replay), so nothing webhook-shaped can surprise you during unrelated test runs.

### Payload shape

Each Notify task receives a single newline-terminated JSON object on **stdin** (never env vars, never argv — those leak via `ps` / shell). Shape:

```json
{
  "schema_version": 1,
  "monad_version": "0.1.0",
  "environment": "staging",
  "trigger": {
    "task_name": "railway:deploy",
    "unit_name": "admin",
    "monad_name": "prod",
    "outcome": "built",
    "exit_code": 0,
    "duration_ms": 4272,
    "cache_key": "12dfe62c9f4c...",
    "integration_kind": "deploy",
    "output_excerpt": "Uploading...\n  Build Logs: https://railway.com/...\n",
    "stderr_excerpt": null
  }
}
```

The schema is published via `monad schema notification-payload` — use it to validate agent-authored notify scripts. `stderr_excerpt` is populated only when `outcome == "failed"`.

### Built-in notifications

Two notifications ship with monad out of the box. Both are opt-in via `unit.toml` — no platform-side file detection.

#### Slack

```toml
# unit.toml
[integrations.slack]
# All fields optional. Defaults assume SLACK_WEBHOOK_URL is set.
webhook_url_env = "SLACK_WEBHOOK_URL"   # override to use per-env names
channel         = "#deploys"            # webhooks pin one at creation — this overrides only if the webhook is unpinned
username        = "Monad"               # optional sender name
```

Emits one `slack:notify` task. Posts a message shaped like:

```
:rocket: *admin* deployed → *staging* in 4.3s (task `railway:deploy`)
<https://railway.com/.../deploy/abc|Build logs>
```

On a failed deploy the emoji flips to `:rotating_light:` and the stderr excerpt is attached as a Slack code block. URL detection pulls the last `https://…` from the deploy's captured output (most CLIs print "Build Logs: …" near the tail).

#### Linear

```toml
# unit.toml
[integrations.linear]
# All fields optional except an env var holding the API key.
api_key_env        = "LINEAR_API_KEY"
target_state       = "Deployed"         # workflow-state name to transition to
fallback_issue_id  = "ENG-1234"         # optional: comment here if no issue refs found
team               = "ENG"              # optional: disambiguate state lookup across teams
```

Emits one `linear:notify` task. On successful deploy, scans the payload for `[A-Z]{2,}-\d+` identifiers (e.g. `ENG-1234`) in the task name / unit name / captured output, then transitions each matched issue to `target_state` via Linear's GraphQL API. When no issues match and `fallback_issue_id` is set, posts a deploy-summary comment on that issue so release visibility isn't lost silently. Failed deploys skip transitions entirely — we don't mark a broken release as shipped.

### Writing your own

**Option A — `[[notifications]]` escape hatch.** For one-off scripts where writing a full Integration is overkill, declare notifications directly in `unit.toml`:

```toml
# unit.toml
[[notifications]]
name         = "github-pr-comment"
run          = "./scripts/notify-github.sh"
env          = ["GITHUB_TOKEN"]
required_env = ["GITHUB_TOKEN"]
required_cli = ["gh: https://cli.github.com"]
```

Each entry becomes a Notify-kind task that fires after every Deploy in the unit with the same fan-out and failure semantics as a built-in notification. The script receives the NotificationPayload JSON on stdin exactly like the built-ins.

Example stdin-consuming script (bash / jq):

```sh
#!/usr/bin/env bash
# notify-github.sh — reads a notification payload on stdin, comments on the PR.
payload="$(cat)"
url="$(jq -r '.trigger.output_excerpt | capture("https://[^\\s]+").string // ""' <<<"$payload")"
unit="$(jq -r '.trigger.unit_name' <<<"$payload")"
env="$(jq -r '.environment // "unknown"' <<<"$payload")"
gh pr comment --body "🚀 \`$unit\` deployed to \`$env\` — [logs]($url)"
```

**Option B — full `Integration` trait.** For reusable integrations (distributed as their own crate or as a subprocess plugin), implement `monad_adapters::Integration` and emit a Notify-kind `IntegrationTask`. Same payload shape on stdin; full access to `required_env` / `required_cli` preflight; opt-in via `[integrations.<id>]`. See `crates/monad-adapters/src/slack.rs` for the minimal template.

### Replaying notifications: `monad notify`

Every `monad deploy` writes a payload sidecar at `.monad/notification/<monad>/<unit>/<task>.json`. `monad notify` reads those sidecars and replays them through Notify tasks without re-running the deploy. The typical flow:

```sh
# 1) Original deploy. Webhook is down, Slack post fails silently.
monad deploy --env prod api
# summary.notify_failures = 1

# 2) Fix the webhook URL / rotate the token / whatever.

# 3) Replay just the notify step.
monad notify --env prod api
```

`.monad/` is already gitignored so sidecars don't leak into commits.

---

## Commands

### `monad deploy [target] [flags]`

Deploy units. Filter semantics:

- **`target`** — monad or unit name. Omit to deploy every unit with a matching integration task.
- **`--env <name>`** — load the named `[environments.<name>]` profile's secret aliases.
- **`--secret-from DECLARED=SOURCE`** — ad-hoc alias (repeatable; overrides `--env`).
- **`--preview`** — run `kind: DeployPreview` tasks (e.g. `vercel:preview`) instead of prod.
- **`--rollback`** — run `kind: Rollback` tasks. Mutually exclusive with `--preview`.
- **`--no-notify`** — skip the Notify-kind notification fan-out after deploy (see `monad notify` below). Use when re-deploying after a fix and you don't want to re-spam Slack / Linear.

The deploy task also runs a `build` precondition. If you want a pure deploy (skip build because you've already built elsewhere), override the task's `depends_on` in `unit.toml`:

```toml
[tasks."railway:deploy"]
depends_on = []
```

### `monad notify [target] [flags]`

Re-fire Notify-kind integration tasks (notifications — Slack / Linear / PagerDuty / custom webhook scripts) using the last deploy's cached payload. Useful when a webhook was misconfigured during the original deploy and you want to re-send once it's fixed, without actually re-running the deploy.

- **`target`** — monad or unit name. Omit to notify every unit with a prior deploy on record.
- **`--env <name>`** — same semantics as `monad deploy --env`; typically pass the same profile you passed to the original deploy so the Slack/Linear tokens resolve.
- **`--secret-from DECLARED=SOURCE`** — ad-hoc alias.

Every `monad deploy` persists a notification payload sidecar at `.monad/notification/<monad>/<unit>/<task>.json` containing the deploy's outcome + captured output. `monad notify` reads those sidecars and pipes them on stdin to each unit's Notify-kind tasks — the same payload shape published by `monad schema notification-payload`. Unites with no sidecar emit a clear `Skipped` row with the message "run `monad deploy` first."

Notify failures never fail the build (exit code stays 0, `summary.notify_failures` tracks them separately). A down Slack webhook shouldn't red-X an otherwise successful deploy.

### `monad doctor --env <name>`

The same preflight `monad deploy` runs internally, surfaceable standalone so you can validate setup without side effects:

```console
$ monad doctor --env staging
  ✓ config                   [ok  ]  1 monad(s), 3 unit(es) loaded
  · toolchain                [skip]  no explicit toolchain pins — nothing to verify
  ✓ integration.railway.env  [ok  ]  all 1 env var(s) present (units: admin, backend, frontend)
  ✓ integration.railway.cli  [ok  ]  all CLI binaries on PATH: railway (units: admin, backend, frontend)
  ✓ cache.local              [ok  ]  ~/.monad/cache: 12 entries, 2.48 MiB
  · cache.remote             [skip]  not configured
  · cache.gha                [skip]  not running inside GitHub Actions
  ✓ git.repo                 [ok  ]  repository reachable
  ✓ git.base_ref             [ok  ]  origin/main → 01c28199169c
```

Agents switch on check names (`integration.<id>.env`, `integration.<id>.cli`, …) — stable, dot-namespaced. Exit non-zero on any `fail`.

---

## GitHub Actions

### One-step deploy via the composite action

```yaml
# .github/workflows/deploy.yml
name: Deploy

on:
  push:
    branches: [main]              # staging on every main push
  release:
    types: [published]            # prod on release

concurrency:
  group: deploy-${{ github.event_name == 'release' && 'prod' || 'staging' }}
  cancel-in-progress: false       # never cancel a prod mid-push

jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Railway CLI
        run: npm install -g @railway/cli
      - uses: thomascarter613/monad-next@v0.1
        with:
          version: '0.1.0'
          task: deploy
          env: ${{ github.event_name == 'release' && 'prod' || 'staging' }}
          install-toolchains: 'false'   # Railway builds server-side
        env:
          RAILWAY_TOKEN_STAGING: ${{ secrets.RAILWAY_TOKEN_STAGING }}
          RAILWAY_TOKEN_PROD:    ${{ secrets.RAILWAY_TOKEN_PROD }}
      - name: Upload monad report
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: monad-report-${{ github.event_name == 'release' && 'prod' || 'staging' }}
          path: ${{ runner.temp }}/monad-report.json
          if-no-files-found: warn
```

What the action does for `task: deploy`:

1. Downloads the monad binary (pinned by `version`).
2. Restores the monad content cache + per-tool dep caches (keeps subsequent deploys fast).
3. Runs `monad doctor --env <env>` as a **preflight** — fails the job with structured output before any real upload starts.
4. Runs `monad deploy --env <env>` with `--report-file <runner.temp>/monad-report.json`.
5. Publishes `report`, `artifacts`, `json` as step outputs — downstream jobs can `jq` the deploy URL directly.

### Inputs specific to deploys

| Input | Description |
|-------|-------------|
| `task` | Set to `deploy`. Default is `ci`. |
| `env` | Named environment profile (see `[environments.<name>]` above). Optional — omit for unaliased env resolution. |
| `secret-from` | Newline-separated `DECLARED=SOURCE` aliases. Overrides `env`. |
| `preview` | `'true'` to run preview deploys. Mutually exclusive with `rollback`. |
| `rollback` | `'true'` to run rollback. Mutually exclusive with `preview`. |
| `target` | Unit or monad name. Omit for all units with matching integrations. |
| `install-toolchains` | `'false'` if your deploy target (like Railway) rebuilds server-side — saves the local toolchain fetch. |

Everything else (caching, structured output, report-file) inherited from the CI action. See [README.md](../README.md#github-action) for the full input table.

---

## Staging / prod split

The canonical split — push-to-main deploys staging, a GitHub Release deploys prod — falls out naturally from the workflow above. Key points:

- **Two separate `[environments.<name>]` profiles** in `monad.toml`, mapping the same declared names to different source env vars (see the secret-aliases section).
- **`concurrency.group`** split by env so a prod release isn't cancelled by an interim main push.
- **Railway's own GitHub integration should be disabled** on services monad is deploying, otherwise both systems race — Railway's integration deploys first, monad's `railway up` becomes a no-op on identical content. Disable per-service via Railway Dashboard → Service → Settings → Source (keep the repo linked for visual context, turn off auto-deploy).

---

## Structured output

Every deploy task's stdout gets captured and surfaced via `ExecutedTask.output_excerpt` (tail-truncated to 4 KB). For Railway, that's where `railway up --ci` prints the build-log URL (and, on success, the tail of the build output):

```console
$ monad deploy --env staging admin
monad: prod (1 unit)

  admin  (node-npm)
    build                 [cache hit ]  ...   6ms
    railway:deploy        [built     ]  ...   4s
      output: Indexing...
              Uploading...
                Build Logs: https://railway.com/project/.../service/.../deploy/abc123
                Deploy URL: ...

summary: 1 unit · 2 tasks · 1 built · 1 cached · 0 failed · 4s
```

Same JSON shape (`monad deploy --json` or via the action's `report` output):

```json
{
  "profiles": [{
    "name": "prod",
    "units": [{
      "name": "admin",
      "tasks": [
        { "name": "build",           "outcome": { "kind": "cache_hit" }, ... },
        { "name": "railway:deploy",  "outcome": { "kind": "built", "exit_code": 0 },
          "output_excerpt": "Indexing...\nUploading...\n  Build Logs: https://...\n" }
      ]
    }]
  }]
}
```

Agents pull the URL from `output_excerpt` without a second `monad why <hash>` call.

---

## Troubleshooting

### "missing required env var(s): RAILWAY_TOKEN (via $RAILWAY_TOKEN_STAGING)"

The alias is resolving but the source env var isn't set. Locally: `export RAILWAY_TOKEN_STAGING=...` (or put it in your shell init). In GHA: add it to the `env:` block of the step, sourcing from `${{ secrets.RAILWAY_TOKEN_STAGING }}`.

### "CLI binary not found on PATH: railway"

The Railway CLI isn't installed where monad can see it. `npm install -g @railway/cli` or `brew install railway`. On a composite GHA, add a `- run: npm install -g @railway/cli` step before the monad action.

### "Could not find root directory: /admin" (from Railway's build logs)

Your Railway service has `rootDirectory: /admin` configured dashboard-side but monad uploaded only the `admin/` subdir as the archive root — Railway looks for `/admin/Dockerfile` inside and can't find it. Fix: set `root = ".."` (or deeper) in `[integrations.railway]` so monad uploads from the monorepo root.

### `railway up`: "prefix not found"

You passed a parent/sibling path as the positional arg to `railway up`. Monad's Railway integration uses `cd <root> && railway up --ci` for exactly this reason; if you see this error, check that your unit.toml's `[integrations.railway] root` is a path relative to the unit dir (not an absolute path).

### Only one service actually got a new deployment

Railway's own GitHub integration is still enabled on the other services — it deployed the same SHA before monad got there, so monad's `railway up` uploaded identical content and Railway reported "no diff." Disable Railway's auto-deploy per service (Dashboard → Service → Settings → Source → disable automatic deployments).

### `monad doctor --env staging` says `integration.railway.env [ok]` but the deploy still fails

The env var is set but empty, or it's for a Railway project / environment scope your token doesn't include. Project tokens are scoped to the specific project + environment they were generated in — a `myapp-staging` token can't deploy to `myapp-prod`. Verify via `railway whoami` (works on account tokens) or `railway status` (works on project tokens).

---

## Related

- [configuration.md](./configuration.md) — every TOML field, including `[environments.<name>]` + `[integrations.<id>]`.
- [new-project.md](./new-project.md) — monad from zero.
- [adopt-existing-repo.md](./adopt-existing-repo.md) — dropping monad into an existing monorepo.
- [plugins.md](./plugins.md) — the subprocess adapter protocol for teaching monad a new language without forking. Integrations can be written against the `monad_adapters::Integration` trait directly, or authored as `[[notifications]]` entries in `unit.toml` for custom post-deploy hooks.
