---
name: monad
description: Use this skill whenever you're working in a repository managed by monad — look for a `monad.toml` at the repo root, a `profiles/` directory, or per-subdir `unit.toml` files. Monad is a polyglot monorepo orchestrator that wraps native package managers (npm / pnpm / yarn / bun / cargo / go / composer / pip / bundle / mvn / gradle / deno) behind one uniform CLI with content-addressed caching, toolchain pinning, and deploy-target routing. On a fresh session, run `monad prime` first to get a workspace snapshot (inventory, cache state, plan preview, recommended next verb). Reach for `monad` verbs (prime / init / migrate / install / ci / build / test / lint / deploy / notify / doctor / why / plan / artifacts / cache / secret / toolchain / release / graph / login / unit list / box list) instead of invoking native package managers — monad handles per-unit scoping, cache layering, and structured output that agents can reason about. **When you need a running service** (smoke test, reproduce a bug, hit an endpoint), start it yourself via `monad dev <unit>` / `monad serve <monad>` in a background shell with logs captured to a file — do NOT probe the user's running processes via `ss` / `lsof` / `pgrep` / `curl localhost:<port>`. Their terminal state isn't your terminal state. Also use when the user mentions monad explicitly, or asks about polyglot monorepo orchestration, content-addressed build caching, notifications (post-deploy hooks), or deploy integrations (Railway, Vercel, Cloudflare Workers, Cloudflare Pages).
---

# Monad — polyglot monorepo orchestrator

## When to reach for monad

If the repo has any of these, the repo is monad-managed and you should prefer `monad` verbs over native package managers:

- `monad.toml` at the repo root
- a `profiles/` directory with one or more `<name>.toml` files
- per-subdir `unit.toml` files declaring a `language`

If monad isn't installed on the host, see the **Installing monad** section below.

## Start with `monad prime`

On a fresh session — or after `/clear` / compaction — run **`monad prime`** before anything else. It prints a concise orientation: profiles, units, cache state, a plan preview, and a recommended next verb. `monad prime --json` emits a schema-stable object registered via `monad schema prime`; agents can switch on `recommended_next[0]` to decide what to do first.

It does not execute tasks, does not hit the network, and runs in under 2 seconds on a cold workspace.

## If an MCP connection is available, prefer `mcp__monad__*` tools

monad ships a `monad-mcp` binary that exposes monad verbs as typed [Model Context Protocol](https://modelcontextprotocol.io) tools. When your client lists tools starting with `mcp__monad__*`, prefer them over shelling out to `monad`:

- `mcp__monad__prime` → same output as `monad prime --json`
- `mcp__monad__plan` → `monad plan --json` (accepts `target`, `monad`, `no_cache`, `since`)
- `mcp__monad__unit_list` / `mcp__monad__box_list` → inventory surface
- `mcp__monad__doctor` → health checks (add `cloud: true` for endpoint probes)
- `mcp__monad__why` → cache-key explanation (`target` accepts `<unit>:<task>` or hex prefix)
- `mcp__monad__artifacts` → resolved output paths per unit
- `mcp__monad__schema` → JSON Schema for any monad output type
- `mcp__monad__install` / `mcp__monad__build` / `mcp__monad__check` / `mcp__monad__test` / `mcp__monad__lint` / `mcp__monad__ci` → execution; mutate `node_modules` / `target/` only. `check` is the fast type-check verb (`cargo check`, `go vet`) — prefer it over `build` while iterating
- `mcp__monad__deploy` / `mcp__monad__notify` → destructive + open-world; MCP client prompts for stronger confirmation. Both require `env` (matching `[environments.<env>]` in monad.toml)

Write-path verbs (`monad unit add` / `monad init` / `monad migrate`) don't have MCP tools yet — shell out via the verb reference below until they land.

Easiest path: `monad mcp install`. With no arguments it auto-detects every installed agent client (Claude Code, Claude Desktop, Cursor, Windsurf, Codex CLI, OpenCode, Zed) and writes the right config for each. Pass a positional client name (`monad mcp install codex`) to register only one. Run `monad mcp install --help` for the full list and exact paths. If the tools aren't listed in `/mcp` (or your client's equivalent), fall back to shell-out via the verb reference below — both produce identical JSON.

## Verb reference

| What you want | Run |
|---------------|-----|
| Orient yourself in a fresh session (inventory + next verb) | `monad prime` |
| List every unit (name, path, language, profiles) + flag orphans | `monad unit list` |
| List every monad (name, source, units) | `monad box list` |
| Scaffold monad.toml + profiles/ in this repo (auto-detects units) | `monad init` |
| Convert a Turborepo workspace into monad config | `monad migrate turbo [--dry-run] [--force]` |
| Convert an Nx workspace into monad config | `monad migrate nx [--dry-run] [--force]` |
| Convert a Lerna workspace into monad config | `monad migrate lerna [--dry-run] [--force]` |
| Convert a Makefile into monad config (best-effort) | `monad migrate make [--dry-run] [--force]` |
| Convert a moonrepo workspace into monad config | `monad migrate moon [--dry-run] [--force]` |
| Convert a Rush.js workspace into monad config | `monad migrate rush [--dry-run] [--force]` |
| Install every unit's deps (replaces `npm ci` / `go mod download` / `composer install` / …) | `monad install` |
| Install one unit | `monad install <unit>` |
| Full CI pass (build + check + test + lint, no deploy/notify) | `monad ci` |
| Build / check / test / lint | `monad build [target]` · `monad check [target]` · `monad test [target]` · `monad lint [target]` |
| Fast type-check across a target (`cargo check --locked --all-targets`, `go vet ./...`) — order of magnitude faster than `monad build` for tight iteration loops | `monad check [target]` |
| Deploy to a named environment | `monad deploy --env <env> [target]` |
| Preview / staging deploy | `monad deploy --preview --env <env>` |
| Rollback | `monad deploy --rollback --env <env>` |
| Force a deploy even when inputs unchanged | `monad deploy --force --env <env>` |
| Re-fire Slack/Linear notifications without re-deploying | `monad notify --env <env> [target]` |
| Run a monad with hot reload | `monad serve <monad>` |
| Run a single unit in dev mode | `monad dev <unit>` |
| Invoke a `[tasks.<name>]` block ad-hoc (CLIs, migrations, one-offs); bypasses the cache | `monad run <unit> <task> -- <args…>` |
| Add a dependency to a unit (cargo / bun / npm / pnpm / yarn / go) | `monad add <pkg>… --unit <d> [--dev]` |
| Show what would run, without running | `monad plan` |
| Explain a cache decision | `monad why <cache-key-prefix>` |
| Print the dependency graph | `monad graph [--format dot]` |
| Resolved artifact paths | `monad artifacts --json` |
| Health check before a deploy | `monad doctor --env <env>` |
| Health check + cloud probes (JWT, cache.monad.build, api.monad.build) | `monad doctor --cloud` |
| Scaffold a new unit | `monad unit add <path> --lang <ecosystem>` |
| Create a new monad (deployment unit) | `monad box add <name>` |
| Cache management | `monad cache stats|clear|push|pull` |
| Manage deploy-target secrets (Cloudflare / Railway) | `monad secret put|list|delete <target> <name>` |
| Toolchain management | `monad toolchain list|install|pin <tool=ver>` |
| Cut a release (bump workspace version, refresh lockfile, commit, tag) | `monad release <patch|minor|major|X.Y.Z>` |
| Sign in to monad.build + stash the cache JWT in OS keychain (or `~/.monad/credentials` 0600 fallback) | `monad login` |

Global flags worth knowing: `--json`, `--no-cache`, `--monad <name>`, `--since <ref>`, `--report-file <path>`, `--skip-install`, `--force-install`, `-v` / `--verbose`.

## Agent-friendly output

Every reporting command supports `--json` with a stable, documented schema. (Streaming verbs — `monad dev`, `monad serve`, `monad run` — pass output straight through, so `--json` is a no-op there.) When reasoning about output:

- Use `--json` (or `--report-file <path>`) instead of parsing stdout.
- Every output type has a JSON Schema: `monad schema [plan|report|why|scaffold|doctor|manifest|error|diagnostics|notification-payload|prime]`. Run `monad schema` with no arg to list available schemas.
- Task failures include structured `diagnostics[]` for compiler / linter errors (cargo, eslint, golangci-lint, ruff). Don't parse tool-specific stderr — read `diagnostics`.
- Cache decisions aren't mysterious: `monad why <hash>` returns the full input manifest behind any cache key (adapter, toolchain version, env-var names, every hashed file + its blake3 digest).
- Integration tasks (Deploy / Notify / Release) capture an `output_excerpt` (4 KB tail of stdout+stderr) on the `ExecutedTask`, so the deploy URL / build-log URL surfaces in both human + JSON output without needing a second call.

## Deploys

The `monad deploy` verb wraps each platform's native CLI (Railway, Vercel, Cloudflare Workers, Cloudflare Pages). Key rules:

- **Run `monad doctor --env <env>` first.** It's a preflight — fails with structured check names (`integration.railway.env`, `integration.railway.cli`, …) before any real upload. Exit non-zero on any `fail`. Add `--cloud` to additionally validate the remote-cache JWT + ping `cache.monad.build/health` + `api.monad.build/v1/healthz`.
- **Never pass secret values on the CLI.** Use `[environments.<name>]` in `monad.toml` for named secret profiles, or `--secret-from DECLARED=SOURCE` for ad-hoc aliasing. The flag rejects literal-looking values at parse time. To set platform secrets, use `monad secret put <unit> NAME` (reads value from stdin).
- **`monad ci` deliberately excludes side-effectful integration tasks** (Deploy / DeployPreview / Rollback / Notify). Deploys only happen via explicit `monad deploy`.
- **Deploys short-circuit when inputs haven't changed.** Monad records the last successful deploy's input manifest in `.monad/state/deploys.json`; subsequent `monad deploy` runs skip Deploy tasks whose inputs match. Override with `monad deploy --force` when you need to re-deploy regardless (e.g. external env changed).
- **Railway adapter uses `railway up --ci`** so non-TTY callers (which is everything monad launches) actually block on the server-side build outcome. Plain `railway up` silently detaches in non-TTY contexts and exits 0 before Railway has built anything — `--ci` is the only correct form.

## Notifications — post-deploy hooks (Slack, Linear, custom)

After every Deploy task, monad fans out **Notify-kind** tasks (notifications) in parallel. They receive a structured `NotificationPayload` JSON on stdin (`monad schema notification-payload`) and **never affect the deploy's exit code** — a broken Slack webhook can't red-X a successful deploy.

Built-ins (config-driven opt-in via `[integrations.<id>]` in `unit.toml`):

- **`[integrations.slack]`** — POSTs a templated message to a Slack Incoming Webhook. Outcome-driven emoji (rocket / siren / package), URL auto-extraction from the deploy's captured output, stderr code-block on failure (2 KB-capped). Config keys: `webhook_url_env`, `channel`, `username`.
- **`[integrations.linear]`** — Scans the payload for `[A-Z]{2,}-\d+` issue refs across task name / unit name / captured output, then transitions each matched issue to a configurable `target_state` via Linear's GraphQL API. Config keys: `api_key_env`, `target_state`, `fallback_issue_id`, `team`. Failed deploys skip transitions (so a broken release is never marked shipped) but still comment on the fallback issue.
- **`[[notifications]]` block** — inline custom Notify-kind tasks (GitHub PR comments, PagerDuty triggers, custom log forwards). Each entry becomes a synthetic Notify task with `env` / `required_env` / `required_cli` preflight.

Use `monad deploy --no-notify` to suppress notifications for a single deploy. Re-fire after a webhook fix without re-deploying with `monad notify --env <env> [target]` (replays the last deploy's sidecar at `.monad/notification/<monad>/<unit>/<task>.json`).

## Build reports — automatic for `monad ci` / `monad build`

When a `monad://` remote is configured in `[cache]` and the token env var is set, `monad ci` and `monad build` POST a `BuildReport` to `<base>/report/build` after the run completes. Same Bearer auth as cache writes; best-effort (failures log a warning, never fail the build). One report per invocation (summary of all units), not per unit. Test/lint runs deliberately don't emit. Self-hosted backends can reject with 404 and monad silently no-ops — the protocol is opt-in for backends that don't care about dashboards. Schema: `monad_cas_protocol::BuildReport` (`package`, `branch`, `sha`, `cache_hit_ratio`, `status`, `duration_ms`).

## Config surface you'll encounter

Three TOML files define a monad workspace:

- **`monad.toml`** at repo root — cache tiers, toolchain pins, `[environments.<name>]` secret profiles.
- **`profiles/<name>.toml`** — one or more — each lists which units ship together.
- **`<unit>/unit.toml`** — per-unit config: `language`, `depends_on`, `[toolchain]` overrides, `[tasks.<name>]` custom recipes, `[integrations.<id>]` deploy config (e.g. Railway service name, Cloudflare project name, Slack webhook env), and `[[notifications]]` blocks for custom post-deploy hooks.

Full reference: `docs/configuration.md`. Deploys + notifications: `docs/deploying.md`. Agent integration patterns: `docs/agents.md`.

## Installing monad

If `monad` isn't on the host:

```sh
curl -fsSL https://monad.build/install | sh
```

Installs the latest release binary to `~/.local/bin/monad`. Set `MONAD_INSTALL_DIR` to override. On first use, run `monad doctor` to verify the workspace.

## When NOT to use monad

Monad isn't for everything. Keep using the right tool for:

- **Exploratory file operations** — `ls`, `cat`, `grep`, your agent's standard file tools.
- **One-off debugging** — `psql`, `curl`, `dig`, etc.
- **Git operations** — monad doesn't wrap git. `gh` and other repo-management tools are fine.
- **Unfamiliar commands already running** — if the user has a script they're attached to, ask before refactoring it through monad.

If in doubt: "is this something that could live in CI?" → prefer monad. Otherwise → native tool.

## Smoke-testing services: start your own, don't probe the user's

When you need to hit a running service — to reproduce a bug, run a curl-shaped smoke test, or watch a log — **start it yourself** in this session. Do not assume the user has `monad dev <unit>` open in another terminal, and do not go hunting for it with `ss` / `lsof` / `pgrep` / `ps` / `curl localhost:<port>`. Their terminal state isn't your terminal state: the process you find may be wedged, stale, missing key env vars, or about to be killed; the logs you'd want are in a pipe you can't read; and you'll mistake "the user already had this running" for "this works."

**The recipe** (works for any HTTP service in a monad workspace):

```bash
# 1. Start the service in the background with logs captured to a file you own.
#    Use `monad dev <unit>` for one unit, `monad serve <monad>` for a bundle.
mkdir -p /tmp/monad-debug
monad dev <unit> > /tmp/monad-debug/<unit>.log 2>&1 &
echo $! > /tmp/monad-debug/<unit>.pid

# 2. Wait for it to be ready. Don't sleep blindly — poll the readiness probe
#    until it answers, with a hard cap.
for i in {1..30}; do
  curl -fsS -m 1 http://127.0.0.1:<port>/healthz >/dev/null 2>&1 && break
  sleep 0.5
done

# 3. Drive the failure. Capture the full response so you have evidence.
curl -sS -w '\n--- HTTP %{http_code} in %{time_total}s ---\n' \
  -X POST http://127.0.0.1:<port>/<route> \
  -H 'Content-Type: application/json' -d '<payload>'

# 4. Read the log file (the one YOU wrote) for the server-side traceback.
tail -n 200 /tmp/monad-debug/<unit>.log

# 5. Tear it down when you're done. Always.
kill "$(cat /tmp/monad-debug/<unit>.pid)" 2>/dev/null
```

**Why this matters.** Probing the user's terminal-running processes is a subtle category of "I'm using their state as my test fixture." It looks innocent (`curl /healthz` is read-only!) but it lies to you in three ways: (1) you can't read the server's stdout because you don't own the pipe, so a 500 has no traceback to attach to; (2) ports being LISTEN doesn't mean the process is healthy — it can be wedged with a backed-up accept queue; (3) you start treating "it works on the user's machine right now" as the source of truth instead of "it works when freshly started under my recipe", which is the only thing CI / the next session / the user-after-a-reboot will see.

**Specific port-probe shapes to avoid** inside a monad workspace:

- `ss -ltnp` / `lsof -iTCP -sTCP:LISTEN` / `netstat -ltnp` to discover what's running
- `pgrep -f vite|wrangler|uvicorn|firebase|next|nuxt|node` (or the same with `ps -ef | grep`)
- `curl http://localhost:<port>/...` against ports you didn't start in this session
- Reading `/proc/<pid>/fd/{1,2}` to recover stdout/stderr from a process you didn't launch

If the user is actively driving a service in another terminal and asks you to "check what it's doing," that's the exception — but say so out loud ("you have <unit> running on :<port> already, I'll hit that directly") so the user can correct you if their terminal has since moved on.

## Anti-patterns: do NOT reach for native tooling inside a monad workspace

These come up over and over. Each row is a real footgun and the monad verb that does it correctly. **Reach for the right column, not the left, even when you're "just checking" or "just debugging."** Diagnostic invocations are not exempt — monad's structured output already carries the diagnostic info you'd otherwise hunt for.

| ❌ Don't run | ✅ Use instead |
|---|---|
| `bun install`, `npm ci`, `pnpm install`, `yarn install`, `pip install`, `uv sync`, `uv pip install` | `monad install [--monad <name>]` |
| `bun add <pkg>`, `npm i <pkg>`, `uv add <pkg>` | `monad add <pkg> --unit <d> [--dev]` |
| `bun test`, `npm test`, `pytest`, `cargo test`, `go test`, `python -m pytest`, `uv run pytest` | `monad test [<unit>]` |
| `bunx tsc --noEmit`, `tsc --noEmit`, `eslint`, `prettier --check`, `ruff check`, `mypy`, `golangci-lint run`, `python -m compileall` | `monad lint [<unit>]` (or `monad check` for the fast path) |
| `bun run build`, `npm run build`, `bunx vite build`, `cargo build`, `go build` | `monad build [<unit>]` |
| `bun run dev`, `vite`, `bunx wrangler dev` | `monad dev <unit>` (or `monad serve <monad>` for the whole monad) |
| `bunx wrangler deploy`, `railway up`, `vercel --prod` | `monad deploy --env <env> [<unit>]` |
| `bunx tsc --version` (or any tool-version probe) | Don't probe. The version is in `monad doctor` and the unit's `[toolchain]`. If you genuinely need it, use `monad toolchain list`. |
| `ss -ltnp`, `lsof -iTCP`, `pgrep -f vite\|wrangler\|uvicorn`, `curl localhost:<port>` to find/hit the user's running services | Start your own via `monad dev <unit>` in the background with logs to a file. See **Smoke-testing services** above. |

The cost of slipping is real:
- **Lost cache** — a native invocation populates the tool's own cache (`.next`, `target/`, `__pycache__`, `node_modules`) but does not register the result in monad's content-addressed cache, so the next `monad ci` re-builds from scratch.
- **Wrong toolchain** — native invocations use whatever's first on `$PATH`, not the version pinned in `monad.toml [toolchain]` or `unit.toml [toolchain]`. Trains future-you to assume the wrong contract.
- **Missing scoping** — `bun install` at the wrong dir installs at the wrong scope; `monad install --monad <name>` installs exactly the units that monad ship together.

If a verb you need genuinely isn't in the list above, file an upstream ask rather than working around it locally — the workaround tends to outlive the missing feature.

## Recommended: install the monad-guard hook

The skill ships a `PreToolUse` hook (`hooks/monad-guard.sh`) that intercepts the Bash tool, detects whether the cwd is inside a monad workspace, and blocks the patterns in the anti-patterns table above with a stderr message naming the right `monad <verb>`. Outside a monad workspace, the hook is a no-op, so it's safe to install user-wide.

**Per-project install** (most conservative — only fires in workspaces that opted in):

1. Drop the hook into the project:

   ```sh
   mkdir -p .claude/hooks
   cp ~/.claude/skills/monad/hooks/monad-guard.sh .claude/hooks/
   chmod +x .claude/hooks/monad-guard.sh
   ```

2. Register it in `.claude/settings.json`:

   ```jsonc
   {
     "hooks": {
       "PreToolUse": [
         {
           "matcher": "Bash",
           "hooks": [
             { "type": "command", "command": "$CLAUDE_PROJECT_DIR/.claude/hooks/monad-guard.sh" }
           ]
         }
       ]
     }
   }
   ```

**User-wide install** (one config, applies in any monad workspace you ever open):

Add the same `PreToolUse` block to `~/.claude/settings.json`, but point at the canonical skill copy so updates flow automatically:

```jsonc
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "$HOME/.claude/skills/monad/hooks/monad-guard.sh" }
        ]
      }
    ]
  }
}
```

The hook walks up from cwd looking for `monad.toml`; outside a monad workspace it exits 0 immediately, so it's safe everywhere.

**Bypass.** If you genuinely need to run a native command (e.g. a one-off debugging probe that monad doesn't cover), prefix the command with `MONAD_GUARD_BYPASS=1 ` and it'll pass through. Reach for this rarely — most "I just need to check X" cases are already covered by `monad doctor`, `monad why`, or `monad <verb> --json`.

**Verifying the install.** Run a known-blocked command in any monad workspace; the hook should refuse with the stderr message. The skill ships a test harness:

```sh
echo '{"tool_input":{"command":"bun install"},"cwd":"<your monad workspace>"}' \
  | ~/.claude/skills/monad/hooks/monad-guard.sh
echo "exit=$?"   # expect 2
```
