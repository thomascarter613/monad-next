# Using monad with coding agents

Monad is built for agents first. This page covers how to wire a coding agent — Claude Code, Claude Desktop, Cursor, Windsurf, Codex CLI, OpenCode, Zed, or any other MCP-speaking client — into a monad-managed repo so the agent uses `monad` verbs instead of rediscovering native tooling every turn.

---

## The problem monad solves for agents

A polyglot monorepo without monad punishes agents the way it punishes humans — only harder. The agent has to:

- Discover which package manager each subdir uses (`package-lock.json`? `pnpm-lock.yaml`? `go.mod`? `composer.json`?)
- Pick the right invocation per subdir (`npm ci` vs `pnpm install --frozen-lockfile` vs `yarn install --immutable` vs `go mod download` vs `composer install`)
- Handle deploy per-platform (`vercel deploy --prod --yes`? `railway up --ci --service X`? `wrangler deploy`?)
- Parse each tool's stdout format — different for `npm run test` vs `go test` vs `pytest`

Every one of these is a token-burn opportunity, and every one can go wrong. Monad collapses them into a small set of uniform verbs:

| Agent wants to… | Without monad | With monad |
|-----------------|---------------|------------|
| Install deps | `npm ci` / `pnpm install --frozen-lockfile` / `go mod download` / `composer install` / … | `monad install` |
| Run CI-like checks | `npm test && npm run lint && go test ./... && …` | `monad ci` |
| Deploy to Railway | `railway up --ci --service <name>` from the right dir | `monad deploy --env <env>` |
| See *why* a task ran or cached | Check file mtimes, parse lockfiles, squint | `monad why <hash>` (JSON) |
| Check if everything's wired up | Read the README, hope you didn't miss a step | `monad doctor --env <env>` |

Every output is JSON-available via `--json`, schemaed via `monad schema <type>`, and stable enough to switch on.

---

## Drop-in `CLAUDE.md` / `AGENTS.md` snippet

Paste this into `CLAUDE.md` at the root of any monad-managed repo. It tells the agent "this repo uses monad, here's the verb surface, prefer it over native tooling."

````markdown
## Build, test, deploy — use `monad`, not native tooling

This repo is managed by **monad** (<https://github.com/thomascarter613/monad-next>). Monad wraps every unit's native package manager (npm / pnpm / yarn / bun / cargo / go / composer / pip / bundle / mvn / gradle / deno) behind one uniform CLI. **Always prefer `monad` verbs over running the native tools directly** — monad handles per-unit scoping, content-addressed caching, toolchain pinning, and deploy-target routing in one step.

### Verb reference

| Task | Command |
|------|---------|
| Orient yourself in a fresh session (inventory + recommended next verb) | `monad prime` |
| Install every unit's deps | `monad install` |
| Single unit only | `monad install <unit>` |
| Full CI pass (build + check + test + lint on everything) | `monad ci` |
| Build one target | `monad build [monad-or-unit]` |
| Fast type-check (`cargo check`, `go vet`, …) | `monad check [monad-or-unit]` |
| Run tests | `monad test [monad-or-unit]` |
| Run linters | `monad lint [monad-or-unit]` |
| Add a dependency to a unit (one or many packages, optionally dev) | `monad add <pkg>… --unit <d> [--dev]` |
| Invoke a custom `[tasks.<name>]` block ad-hoc (CLIs, migrations, one-off scripts) | `monad run <unit> <task> -- <args…>` |
| Deploy to staging | `monad deploy --env staging` |
| Deploy to prod | `monad deploy --env prod` |
| Preview deploy | `monad deploy --preview --env staging` |
| Re-send Slack / Linear notifications without re-deploying | `monad notify --env <env> [target]` |
| Explain a cache decision | `monad why <cache-key-prefix>` |
| Health check (config + toolchains + integrations) | `monad doctor` |
| Show what would run without running it | `monad plan` |
| Show resolved artifact paths per unit | `monad artifacts --json` |

### Hot tips

- **Start with `monad prime`.** It's purpose-built for session-start: workspace inventory, cache state, plan preview, and a recommended next verb in one command. `monad prime --json` has a schema-stable shape (`monad schema prime`); `recommended_next[0]` is the first thing to do.
- **Always pass `--json` when you want to reason about the output.** Every reporting command emits structured JSON via the flag; shapes are stable and documented via `monad schema <target>`. Streaming verbs (`monad dev`, `monad serve`, `monad run`) pass through to the wrapped process — `--json` is a no-op there.
- **Don't read stderr to decide what went wrong on a failed task.** Use `monad ci --json` — the `executedTask.outcome` tagged union has `kind: "failed"` with `exit_code` and `stderr_excerpt`, plus structured `diagnostics[]` for compiler / linter errors when available.
- **If something looks miscached or wrongly-built, use `monad why <hash>`** rather than guessing. It returns the full input manifest (every hashed file with its blake3 digest, toolchain version, env-var names, …). The cache key itself is visible on every task in the report.
- **Before a `monad deploy`, run `monad doctor --env <env>`** first — the preflight fails fast with structured check names (`integration.railway.env`, `integration.railway.cli`, …) so you know which knob to tweak.
- **Never pass secret values on the CLI.** Use `[environments.<name>]` in `monad.toml` for named secret profiles, or `--secret-from DECLARED=SOURCE` for ad-hoc aliasing. The flag rejects literal-looking values so an accidental `--secret-from TOKEN=rlw_abc123` errors at parse time.
- **Check if monad itself knows about the work you're about to do.** Unfamiliar task names? `monad plan` shows the resolved task list for every unit. Unfamiliar unit? `monad unit add <path>` scaffolds one.

### Installing monad (if the binary isn't on the host yet)

```sh
curl -fsSL https://monad.build/install | sh
```

Or pin a version:

```sh
curl -fsSL https://monad.build/install | sh -s -- 0.1.0
```

Installs both `monad` and `monad-mcp` to `~/.local/bin/` by default, plus the Claude Code skill bundle under `~/.claude/skills/monad/`. Set `MONAD_INSTALL_DIR` to override the binary path or `MONAD_SKILL_DIR` to override the skill path.
````

Drop that block in as-is. Most coding agents — Claude Code, Claude Desktop, Cursor, Windsurf, Codex CLI, OpenCode, Zed, … — scan top-level markdown files on session start and treat them as persistent instructions.

---

## What the snippet does

- **Vocabulary anchoring.** Naming monad up front stops the agent from rediscovering "oh, this is a monorepo, I should run npm on one subdir and go on another."
- **Verb table.** The agent already has context for what `npm test` does. Giving them the monad-equivalent in the same shape is enough for them to map the intent across without reinvention.
- **`--json` pointer.** Agents that pipe stdout through string parsing waste tokens on brittle regex. Pointing them at `--json` + `monad schema <target>` gives them stable, declarative access to every decision.
- **`monad why` as the "ask" rather than the "guess".** Agents tend to guess why a build was rebuilt ("probably the dependency changed"). `monad why <hash>` returns the authoritative answer.
- **Secret-handling rule.** The literal-value rejection at the flag parser catches accidental leaks but the agent should learn the pattern. Spelling out "never pass secret values on the CLI" saves a follow-up correction.

---

## When your agent *shouldn't* use monad

Not every command needs to flow through monad. The snippet nudges toward monad but shouldn't block the agent from:

- **Exploring the repo** — `ls`, `cat`, `grep` (or the agent's equivalents) to understand structure.
- **One-off debugging commands** — e.g. `psql` to inspect a dev database, `curl` to probe an API.
- **Git operations** — monad doesn't wrap git and shouldn't.

A good mental rule: "if the agent is about to do something that could've been part of CI, prefer monad; otherwise use whatever fits."

---

## MCP server — `monad-mcp` (preferred for agents)

monad ships a second binary, `monad-mcp`, that exposes every monad verb as a typed [Model Context Protocol](https://modelcontextprotocol.io) tool. Clients that speak MCP — Claude Code, Claude Desktop, Cursor, Windsurf, Codex CLI, OpenCode, and Zed — auto-discover the tools and invoke them directly: no shell-out, no stdout parsing, no per-repo `CLAUDE.md` snippet. The tool outputs match `monad <verb> --json` byte-for-byte.

Install monad as usual (`curl | sh`); `monad-mcp` lands on `PATH` next to `monad`. Then register it in whichever clients you use:

```sh
monad mcp install                # auto-detect every installed client and register
monad mcp install claude-code    # one client at a time (positional arg)
monad mcp install codex --local  # project-scoped (Codex trusted-projects flow)
```

`monad mcp install --help` lists every supported client and the file it writes.

### Claude Desktop

Edit `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or the equivalent on Windows / Linux:

```json
{
  "mcpServers": {
    "monad": {
      "command": "monad-mcp",
      "args": ["--workspace", "/abs/path/to/your/repo"]
    }
  }
}
```

Restart Claude Desktop. The `mcp__monad__*` tools appear in the tool picker — grouped by capability:

- **Read-only** (no confirmation needed): `monad_prime`, `monad_plan`, `monad_unit_list`, `monad_box_list`, `monad_doctor`, `monad_why`, `monad_artifacts`, `monad_schema`.
- **Execution** (mutates `node_modules` / `target/`): `monad_install`, `monad_build`, `monad_check`, `monad_test`, `monad_lint`, `monad_ci`.
- **Destructive + open-world** (client shows stronger confirmation): `monad_deploy`, `monad_notify`.

Write-path tools (`monad_unit_add`, `monad_init`, `monad_migrate`) are a follow-up — their CLI modules need to land in `monad-core` first.

### Claude Code

Run `monad mcp install claude-code` (or `monad mcp install claude-code --local` for project scope). The installer writes:

- User-global → `~/.claude.json` (single dotfile holding all Claude Code state, including `mcpServers`).
- Project-local → `.mcp.json` at the repo root (Claude Code reads this when it lives next to `.claude/settings.json`).

If you'd rather write the file by hand, the entry shape is:

```json
{
  "mcpServers": {
    "monad": {
      "command": "monad-mcp",
      "env": { "MONAD_WORKSPACE_ROOT": "${workspaceFolder}" }
    }
  }
}
```

Claude Code renders the tools as `mcp__monad__<verb>` — check via `/mcp` after connecting.

### Worked example

End-to-end flow an agent follows in a fresh session:

```
1. agent → mcp__monad__prime
   ← {workspace_root, profiles: [...], units: [...], plan: {hits, misses},
      recommended_next: ["6 task(s) would miss cache — run `monad ci` ..."]}

2. agent sees misses, calls mcp__monad__plan
   ← {profiles: [{name, units: [{name, tasks: [{name, status: "cache_miss",
      miss_reason: "never_cached", key: "73f616..."}, ...]}]}]}

3. agent picks a specific miss it wants to understand:
   mcp__monad__why {target: "marketing:lint"}
   ← {key, manifest: {files: [{path, blake3, size_bytes}, ...],
      env_vars: [...], toolchain: "bun@1.1.30"}}
```

No shell, no stdout-parsing, every step returns structured JSON the agent's tool-call handling already understands.

### Server lifetime + `--workspace` resolution

`monad-mcp` is a single-workspace stdio server — launch one per repo. Workspace resolves in order: `--workspace <PATH>` flag > `$MONAD_WORKSPACE_ROOT` env > current working directory (walking upward for `monad.toml` / `profiles/`). Agents that manage multiple repos should add multiple entries to their MCP client config — one per repo.

---

## Claude Code skill (auto-installed by `install.sh`)

Monad ships a [Claude Code skill](../skills/monad/SKILL.md) that activates automatically when the agent is working in a monad-managed repo — no `CLAUDE.md` snippet required per repo. **The official installer drops the skill under `~/.claude/skills/monad/` for you** — if you ran `curl -fsSL https://monad.build/install | sh`, you already have it.

To verify or update by hand:

```sh
ls ~/.claude/skills/monad/SKILL.md          # should exist
MONAD_FORCE_SKILL=1 curl -fsSL https://monad.build/install | sh   # re-fetch from the latest release tarball
```

If you'd rather grab the file directly without re-running the installer:

```sh
mkdir -p ~/.claude/skills/monad
curl -fsSL https://raw.githubusercontent.com/thomascarter613/monad-next/main/skills/monad/SKILL.md \
  -o ~/.claude/skills/monad/SKILL.md
```

After that, Claude Code auto-loads the skill when it sees a `monad.toml` / `profiles/` / `unit.toml` in the workspace. The skill covers the same verb reference as the `CLAUDE.md` snippet above, plus deploy preflight rules, secret handling, and when-not-to-use guidance.

If you prefer not to install the skill globally, the per-repo `CLAUDE.md` snippet above is equivalent — drop it into any repo that monad manages.

## Related

- [configuration.md](./configuration.md) — every TOML field.
- [deploying.md](./deploying.md) — monad's deploy verbs + secret handling in depth.
- [adopt-existing-repo.md](./adopt-existing-repo.md) — dropping monad into an existing monorepo.
- [new-project.md](./new-project.md) — monad from zero.
- [plugins.md](./plugins.md) — subprocess adapter protocol (for languages monad doesn't know about yet).
