#!/usr/bin/env bash
#
# monad-guard.sh — PreToolUse hook for the Bash tool.
#
# Steers the agent to `monad <verb>` instead of native package managers
# whenever the working directory is inside a monad workspace (detected by
# walking up looking for a monad.toml).
#
# Behaviour:
#   - Outside a monad workspace: pass through (exit 0).
#   - Inside a monad workspace: if the command matches a known native-tool
#     pattern (bun install, pnpm test, pip install, pytest, uv sync, cargo
#     build, go test, bunx tsc, npx vite, …), block it (exit 2) with a
#     stderr message naming the monad verb to use.
#   - On parsing trouble (missing jq, malformed input, etc.): fail-safe
#     and allow. Hooks must never silently break the agent's workflow.
#
# Bypass:
#   - Prefix the command with `MONAD_GUARD_BYPASS=1 ` to skip the guard
#     for one shot. Reserve this for genuine emergencies; the right
#     habit is to use the monad verb.
#   - Setting MONAD_GUARD_BYPASS=1 in the parent environment also works.
#
# Install:
#   See ~/.claude/skills/monad/SKILL.md (section "Recommended: install
#   the monad-guard hook"). The short form is to register this script
#   under hooks.PreToolUse[].hooks[] for the Bash matcher in either the
#   project's .claude/settings.json or the user-level ~/.claude/settings.json.

set -uo pipefail

# ---------------------------------------------------------------------------
# Read tool input from stdin. The PreToolUse payload is JSON with at least:
#   { "tool_name": "Bash", "tool_input": { "command": "..." }, "cwd": "..." }
# ---------------------------------------------------------------------------
input=$(cat 2>/dev/null || true)
[[ -z "$input" ]] && exit 0

# Fail-safe: without jq we can't parse the payload reliably. Allow.
if ! command -v jq >/dev/null 2>&1; then
  exit 0
fi

command=$(printf '%s' "$input" | jq -r '.tool_input.command // empty' 2>/dev/null)
cwd=$(printf '%s' "$input" | jq -r '.cwd // empty' 2>/dev/null)

[[ -z "$command" ]] && exit 0
[[ -z "$cwd" ]] && cwd=$(pwd 2>/dev/null || echo "")

# ---------------------------------------------------------------------------
# Bypass.
# ---------------------------------------------------------------------------
[[ "${MONAD_GUARD_BYPASS:-}" == "1" ]] && exit 0
# Match any leading env-var assignment that includes MONAD_GUARD_BYPASS=1.
if [[ "$command" =~ (^|[[:space:]])MONAD_GUARD_BYPASS=1([[:space:]]|$) ]]; then
  exit 0
fi

# ---------------------------------------------------------------------------
# Workspace detection: walk up from cwd looking for monad.toml. If not
# found, this isn't a monad workspace — pass through.
# ---------------------------------------------------------------------------
monad_root=""
dir="$cwd"
while [[ -n "$dir" && "$dir" != "/" ]]; do
  if [[ -f "$dir/monad.toml" ]]; then
    monad_root="$dir"
    break
  fi
  dir=$(dirname "$dir")
done
[[ -z "$monad_root" ]] && exit 0

# ---------------------------------------------------------------------------
# Pattern table. Each entry is "<regex>@@@<suggestion>" (using @@@ as a
# separator that won't collide with the `|` literals inside either field).
# The regex is tested against the entire command via [[ =~ ]]; the
# suggestion is the monad verb the agent should use instead.
#
# Order matters — first match wins, so put more-specific patterns above
# more-general ones.
# ---------------------------------------------------------------------------
patterns=(
  # Workspace install (npm/pnpm/yarn/bun/pip/uv/composer/bundle/mvn/gradle/deno).
  '(^|[;&|[:space:]])(bun|npm|pnpm|yarn)[[:space:]]+(install|ci|i)\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])pip[[:space:]]+install\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])python[0-9.]*[[:space:]]+-m[[:space:]]+pip[[:space:]]+install\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])python[0-9.]*[[:space:]]+setup\.py[[:space:]]+(install|develop)\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])uv[[:space:]]+(sync|pip)\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])composer[[:space:]]+(install|update)\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])bundle[[:space:]]+(install|update)\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])mvn[[:space:]]+dependency:resolve\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])(\./)?gradlew?[[:space:]]+dependencies\b@@@monad install [--monad <name>]'
  '(^|[;&|[:space:]])deno[[:space:]]+install\b@@@monad install [--monad <name>]'

  # Publish (cargo / npm family / deno) — always destructive, always
  # outside monad. Block them so a release accidentally going through
  # the wrong tool doesn't slip past.
  '(^|[;&|[:space:]])cargo[[:space:]]+publish\b@@@monad release <spec>  (publishes via tag-on-bump CI)'
  '(^|[;&|[:space:]])(bun|npm|pnpm|yarn)[[:space:]]+publish\b@@@monad release <spec>  (or use the platform release flow)'
  '(^|[;&|[:space:]])deno[[:space:]]+publish\b@@@monad release <spec>  (or use the platform release flow)'

  # Cargo install of an arbitrary crate — almost always means "I want
  # this binary on PATH for development", which monad doesn't manage.
  # Surface it so the agent can confirm before bypassing toolchain
  # pinning.
  '(^|[;&|[:space:]])cargo[[:space:]]+install[[:space:]]+[a-zA-Z0-9_-]+@@@(this is a host-level install, not a monad op — confirm with the user before running)'

  # Add / remove deps.
  '(^|[;&|[:space:]])(bun|npm|pnpm|yarn)[[:space:]]+(add|remove|uninstall)\b@@@monad add <pkg> --unit <d> [--dev]'
  '(^|[;&|[:space:]])uv[[:space:]]+(add|remove)\b@@@monad add <pkg> --unit <d> [--dev]'
  '(^|[;&|[:space:]])composer[[:space:]]+(require|remove)\b@@@monad add <pkg> --unit <d> [--dev]'
  '(^|[;&|[:space:]])cargo[[:space:]]+(add|remove)\b@@@monad add <pkg> --unit <d> [--dev]'
  '(^|[;&|[:space:]])go[[:space:]]+get\b@@@monad add <pkg> --unit <d>'

  # Build / typecheck.
  '(^|[;&|[:space:]])(bun|npm|pnpm|yarn)[[:space:]]+run[[:space:]]+(build|compile|typecheck|tsc)\b@@@monad build <unit>'
  '(^|[;&|[:space:]])(bun|npm|pnpm|yarn)[[:space:]]+(build)\b@@@monad build <unit>'
  '(^|[;&|[:space:]])cargo[[:space:]]+(build|check)\b@@@monad (build|check) <unit>'
  '(^|[;&|[:space:]])go[[:space:]]+(build|vet)\b@@@monad (build|check) <unit>'
  '(^|[;&|[:space:]])uv[[:space:]]+build\b@@@monad build <unit>'
  '(^|[;&|[:space:]])(\./)?gradlew?[[:space:]]+(build|assemble|compileJava|compileKotlin)\b@@@monad build <unit>'
  '(^|[;&|[:space:]])mvn[[:space:]]+(compile|package|install|verify)\b@@@monad build <unit>'
  '(^|[;&|[:space:]])(bunx|npx)[[:space:]]+tsc\b@@@monad (lint|build) <unit>'
  '(^|[;&|[:space:]])(bunx|npx)[[:space:]]+vite([[:space:]]+build)?\b@@@monad (build|dev) <unit>'
  '(^|[;&|[:space:]])tsc[[:space:]]+--noEmit\b@@@monad lint <unit>'

  # Test.
  '(^|[;&|[:space:]])(bun|npm|pnpm|yarn)[[:space:]]+(test|run[[:space:]]+test)\b@@@monad test <unit>'
  '(^|[;&|[:space:]])bun[[:space:]]+test\b@@@monad test <unit>'
  '(^|[;&|[:space:]])pytest\b@@@monad test <unit>'
  '(^|[;&|[:space:]])python[0-9.]*[[:space:]]+-m[[:space:]]+pytest\b@@@monad test <unit>'
  '(^|[;&|[:space:]])uv[[:space:]]+run[[:space:]]+pytest\b@@@monad test <unit>'
  '(^|[;&|[:space:]])cargo[[:space:]]+test\b@@@monad test <unit>'
  '(^|[;&|[:space:]])go[[:space:]]+test\b@@@monad test <unit>'
  '(^|[;&|[:space:]])deno[[:space:]]+test\b@@@monad test <unit>'
  '(^|[;&|[:space:]])bundle[[:space:]]+exec[[:space:]]+(rake[[:space:]]+test|rspec|test-unit|minitest)\b@@@monad test <unit>'
  '(^|[;&|[:space:]])(rspec|rake[[:space:]]+test)\b@@@monad test <unit>'
  '(^|[;&|[:space:]])(\./)?gradlew?[[:space:]]+test\b@@@monad test <unit>'
  '(^|[;&|[:space:]])mvn[[:space:]]+test\b@@@monad test <unit>'
  '(^|[;&|[:space:]])(\./)?vendor/bin/(phpunit|pest)\b@@@monad test <unit>'
  '(^|[;&|[:space:]])composer[[:space:]]+test\b@@@monad test <unit>'

  # Lint.
  '(^|[;&|[:space:]])(bunx|npx)[[:space:]]+(eslint|prettier)\b@@@monad lint <unit>'
  '(^|[;&|[:space:]])uvx[[:space:]]+(ruff|mypy)\b@@@monad lint <unit>'
  '(^|[;&|[:space:]])(ruff|mypy|eslint|prettier|golangci-lint)[[:space:]]+(check|run|--check)\b@@@monad lint <unit>'
  '(^|[;&|[:space:]])python[0-9.]*[[:space:]]+-m[[:space:]]+compileall\b@@@monad lint <unit>'
  '(^|[;&|[:space:]])(\./)?gradlew?[[:space:]]+(check|spotlessCheck|ktlintCheck)\b@@@monad lint <unit>'
  '(^|[;&|[:space:]])bundle[[:space:]]+exec[[:space:]]+rubocop\b@@@monad lint <unit>'
  '(^|[;&|[:space:]])(\./)?vendor/bin/(phpstan|psalm|php-cs-fixer)\b@@@monad lint <unit>'

  # Dev / serve.
  '(^|[;&|[:space:]])(bun|npm|pnpm|yarn)[[:space:]]+run[[:space:]]+dev\b@@@monad dev <unit>  (or monad serve <monad>)'
  '(^|[;&|[:space:]])(bunx|npx)[[:space:]]+wrangler[[:space:]]+dev\b@@@monad dev <unit>  (declare a [serve] block in the unit.toml)'
  '(^|[;&|[:space:]])vite([[:space:]]+--port|[[:space:]]+dev)\b@@@monad dev <unit>'
  '(^|[;&|[:space:]])deno[[:space:]]+(task[[:space:]]+dev|run[[:space:]]+--watch)\b@@@monad dev <unit>'

  # Deploy / publish.
  '(^|[;&|[:space:]])(bunx|npx)[[:space:]]+wrangler[[:space:]]+(deploy|publish)\b@@@monad deploy --env <env>'
  '(^|[;&|[:space:]])railway[[:space:]]+up\b@@@monad deploy --env <env>'
  '(^|[;&|[:space:]])vercel[[:space:]]+(deploy|--prod)\b@@@monad deploy --env <env>'
)

# ---------------------------------------------------------------------------
# Match.
# ---------------------------------------------------------------------------
matched=""
suggestion=""
for entry in "${patterns[@]}"; do
  regex="${entry%@@@*}"
  hint="${entry##*@@@}"
  if [[ "$command" =~ $regex ]]; then
    matched="${BASH_REMATCH[0]}"
    suggestion="$hint"
    break
  fi
done

[[ -z "$matched" ]] && exit 0

# ---------------------------------------------------------------------------
# Block. The stderr text is what the agent reads — make it instruction-
# shaped so future calls go through monad.
# ---------------------------------------------------------------------------
cat >&2 <<EOF
[monad-guard] Blocked native-tool invocation inside a monad workspace.

  Workspace:  $monad_root
  Command:    $(printf '%s' "$command" | head -c 200)
  Matched:    ${matched# }

This monorepo is managed by monad. Native package-manager invocations
bypass monad's content-addressed cache, toolchain pinning, and per-unit
scoping. Use the monad verb instead:

  ${suggestion}

Reference:    ~/.claude/skills/monad/SKILL.md
Bypass once:  prefix the command with 'MONAD_GUARD_BYPASS=1 ' (only for
              genuine one-offs that monad doesn't cover).
EOF
exit 2
