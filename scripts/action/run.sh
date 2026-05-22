#!/usr/bin/env bash
# monad action runner — one entry point per phase of the composite
# action (install binary, install toolchains, preflight, execute).
#
# Called from `.github/actions/run` via `runs: using: composite`.
# All phase-specific inputs are routed through environment variables
# (not shell substitution) so injection-shaped inputs can't shell out.
#
# Portability: bash 3.2+ (macOS runners). Namerefs (`local -n`) are
# avoided; functions mutate a shared `MONAD_ARGS` global instead.

set -euo pipefail

PHASE="${1:-}"
if [ -z "$PHASE" ]; then
    echo "usage: run.sh <install-monad|install-toolchains|preflight|execute>" >&2
    exit 2
fi

# ── Shared helpers ─────────────────────────────────────────────────

# Global that build_monad_args / add_secret_from_flags append to.
# Each phase that uses it resets the array at entry.
MONAD_ARGS=()

# Parse $MONAD_SECRET_FROM (newline-delimited DECLARED=SOURCE) and
# append --secret-from flags to MONAD_ARGS. Blank lines + whitespace
# are tolerated; no validation beyond "not empty" — `monad`'s own
# parser rejects malformed values with a clear error.
add_secret_from_flags() {
    local raw line
    while IFS= read -r raw; do
        line="$(printf '%s' "$raw" | awk '{$1=$1};1')"
        [ -z "$line" ] && continue
        MONAD_ARGS+=("--secret-from" "$line")
    done <<< "${MONAD_SECRET_FROM:-}"
}

# Write a KEY/VALUE pair to $GITHUB_OUTPUT using the heredoc form —
# safe for multi-line values (JSON reports) that would otherwise
# truncate on the first newline.
publish_output() {
    local key="$1"
    local value="$2"
    {
        printf '%s<<__MONAD_EOF__\n' "$key"
        printf '%s\n' "$value"
        printf '__MONAD_EOF__\n'
    } >> "$GITHUB_OUTPUT"
}

# Populate MONAD_ARGS for the given $MONAD_TASK. Covers argv shared
# across the CI / build / check / test / lint / deploy verbs, plus
# deploy's extra flag set. Callers append anything task-unrelated
# (e.g. --report-file) after this returns.
build_monad_args() {
    MONAD_ARGS=()
    case "${MONAD_TASK:-}" in
        ci)     MONAD_ARGS+=("ci") ;;
        build)  MONAD_ARGS+=("build") ;;
        check)  MONAD_ARGS+=("check") ;;
        test)   MONAD_ARGS+=("test") ;;
        lint)   MONAD_ARGS+=("lint") ;;
        deploy) MONAD_ARGS+=("deploy") ;;
        notify) MONAD_ARGS+=("notify") ;;
        *)
            echo "::error::unknown task '${MONAD_TASK:-}' (expected one of: ci, build, check, test, lint, deploy, notify)" >&2
            exit 1
            ;;
    esac

    if [ "${MONAD_TASK}" = "deploy" ]; then
        if [ "${MONAD_PREVIEW:-false}" = "true" ] && [ "${MONAD_ROLLBACK:-false}" = "true" ]; then
            echo "::error::preview and rollback are mutually exclusive" >&2
            exit 1
        fi
        if [ -n "${MONAD_ENV:-}" ]; then
            MONAD_ARGS+=("--env" "$MONAD_ENV")
        fi
        add_secret_from_flags
        [ "${MONAD_PREVIEW:-false}"   = "true" ] && MONAD_ARGS+=("--preview")
        [ "${MONAD_ROLLBACK:-false}"  = "true" ] && MONAD_ARGS+=("--rollback")
        [ "${MONAD_NO_NOTIFY:-false}" = "true" ] && MONAD_ARGS+=("--no-notify")
    fi

    # `notify` shares deploy's secret surface (Slack webhook tokens
    # etc.) but none of its preview/rollback/no-notify toggles.
    if [ "${MONAD_TASK}" = "notify" ]; then
        if [ -n "${MONAD_ENV:-}" ]; then
            MONAD_ARGS+=("--env" "$MONAD_ENV")
        fi
        add_secret_from_flags
    fi

    # Positional target applies to every non-ci task. `ci` is
    # whole-workspace by design.
    if [ -n "${MONAD_TARGET:-}" ] && [ "$MONAD_TASK" != "ci" ]; then
        MONAD_ARGS+=("$MONAD_TARGET")
    fi

    # --monad filter applies to every verb.
    if [ -n "${MONAD_NAME:-}" ]; then
        MONAD_ARGS+=("--monad" "$MONAD_NAME")
    fi
}

# ── Phases ─────────────────────────────────────────────────────────

phase_install_profile() {
    mkdir -p "$MONAD_INSTALL_DIR"
    local tag="v${MONAD_VERSION}"

    local arch triple
    case "$(uname -m)" in
        x86_64|amd64)  arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *)             echo "::error::unsupported arch $(uname -m)" >&2; exit 1 ;;
    esac
    case "$(uname -s)" in
        Linux)  triple="${arch}-unknown-linux-gnu" ;;
        Darwin) triple="${arch}-apple-darwin" ;;
        *)      echo "::error::unsupported OS $(uname -s)" >&2; exit 1 ;;
    esac

    local asset="monad-${MONAD_VERSION}-${triple}"
    local tmp
    tmp="$(mktemp -d)"

    echo "==> downloading $asset from release $tag"
    gh release download "$tag" \
        --repo "$MONAD_REPO" \
        --pattern "${asset}.tar.gz" \
        --pattern "${asset}.tar.gz.sha256" \
        --dir "$tmp"

    if [ -f "$tmp/${asset}.tar.gz.sha256" ]; then
        local expected actual
        expected="$(awk '{print $1}' "$tmp/${asset}.tar.gz.sha256")"
        actual="$(sha256sum "$tmp/${asset}.tar.gz" | awk '{print $1}')"
        if [ "$expected" != "$actual" ]; then
            echo "::error::checksum mismatch (expected $expected, got $actual)" >&2
            exit 1
        fi
        echo "==> checksum verified"
    fi

    tar -xzf "$tmp/${asset}.tar.gz" -C "$tmp"
    mv "$tmp/${asset}/monad" "$MONAD_INSTALL_DIR/monad"
    chmod +x "$MONAD_INSTALL_DIR/monad"
    echo "$MONAD_INSTALL_DIR" >> "$GITHUB_PATH"
    "$MONAD_INSTALL_DIR/monad" --version
}

phase_install_toolchains() {
    # monad-toolchain has built-in installers for Go, Node, Python
    # (delegated to `uv python install`), and uv itself (declared
    # co-required by the python tool, so a `[toolchain] python = "..."`
    # pin lays uv down first automatically). Bun and Deno don't have
    # built-in installers yet — we bootstrap those from their upstream
    # install scripts when the workspace pins them.
    bootstrap_external_toolchains

    # Capture stdout + exit code so we can publish the JSON output
    # even on partial failure, then propagate the failure upstream.
    local install_exit=0 json
    json="$(monad toolchain install --json)" || install_exit=$?
    printf '%s\n' "$json"
    publish_output "json" "$json"
    exit "$install_exit"
}

# Read a `<key> = "<value>"` line out of monad.toml's `[toolchain]`
# block. Echoes the value (no quotes) or nothing if absent. Tolerates
# whitespace; ignores commented-out lines. Pure bash so it works on
# both Linux + macOS runners (no GNU-awk dependency).
read_toolchain_pin() {
    local key="$1"
    local file="monad.toml"
    [ -f "$file" ] || return 0

    local in_block=0 line
    while IFS= read -r line; do
        if [[ "$line" =~ ^[[:space:]]*\[toolchain\][[:space:]]*$ ]]; then
            in_block=1
            continue
        fi
        if [[ "$line" =~ ^[[:space:]]*\[ ]]; then
            in_block=0
            continue
        fi
        [ "$in_block" -eq 1 ] || continue
        # Skip commented lines (anywhere a # appears with only whitespace
        # before it, the line is a comment).
        [[ "$line" =~ ^[[:space:]]*# ]] && continue
        if [[ "$line" =~ ^[[:space:]]*${key}[[:space:]]*=[[:space:]]*\"([^\"]*)\" ]]; then
            printf '%s\n' "${BASH_REMATCH[1]}"
            return 0
        fi
    done < "$file"
}

bootstrap_external_toolchains() {
    # Monad's built-in installer covers go, node, python, and uv. Bun
    # and Deno fall through to upstream install scripts for now —
    # tracked separately for proper BunTool / DenoTool support in
    # monad-toolchain (needs zip-archive support).
    local bun_version
    bun_version="$(read_toolchain_pin bun || true)"

    if [ -n "$bun_version" ]; then
        if ! command -v bun >/dev/null 2>&1; then
            install_bun "$bun_version"
        fi
    fi
}

install_bun() {
    local version="$1"
    local install_dir="${HOME}/.bun"

    echo "==> bootstrapping bun (pinned: $version)"
    # bun.sh/install respects BUN_INSTALL for the install root and accepts
    # `bun-v<version>` as the second argument to pin.
    BUN_INSTALL="$install_dir" \
        sh -c 'curl -fsSL https://bun.sh/install | bash -s "bun-v'"$version"'"' \
        >/dev/null
    echo "$install_dir/bin" >> "$GITHUB_PATH"
    export PATH="$install_dir/bin:$PATH"
    "$install_dir/bin/bun" --version
}

phase_preflight() {
    MONAD_ARGS=("doctor")
    if [ -n "${MONAD_ENV:-}" ]; then
        MONAD_ARGS+=("--env" "$MONAD_ENV")
    fi
    add_secret_from_flags
    monad "${MONAD_ARGS[@]}"
}

phase_execute() {
    build_monad_args

    # --report-file always set so the `report` step output is
    # populated regardless of the human-vs-JSON stdout choice.
    MONAD_ARGS+=("--report-file" "$REPORT_FILE")

    local monad_exit=0
    if [ "${MONAD_JSON:-false}" = "true" ]; then
        local json
        json="$(monad "${MONAD_ARGS[@]}" --json)" || monad_exit=$?
        printf '%s\n' "$json"
        publish_output "json" "$json"
    else
        monad "${MONAD_ARGS[@]}" || monad_exit=$?
    fi

    # `report` output: read from --report-file, may be absent on crash.
    if [ -f "$REPORT_FILE" ]; then
        local report
        report="$(cat "$REPORT_FILE")"
        publish_output "report" "$report"
    fi

    # `artifacts` output: best-effort. Never fail the build; always
    # publish valid JSON so downstream `jq` doesn't choke.
    local artifacts
    if ! artifacts="$(monad artifacts --json 2>/dev/null)"; then
        artifacts='{}'
    fi
    publish_output "artifacts" "$artifacts"

    exit "$monad_exit"
}

# ── Dispatch ───────────────────────────────────────────────────────

case "$PHASE" in
    install-monad)       phase_install_profile ;;
    install-toolchains)  phase_install_toolchains ;;
    preflight)           phase_preflight ;;
    execute)             phase_execute ;;
    *)
        echo "::error::unknown phase '$PHASE' (expected: install-monad, install-toolchains, preflight, execute)" >&2
        exit 2
        ;;
esac
