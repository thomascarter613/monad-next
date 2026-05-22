//! Parsers that turn captured tool output into [`Diagnostic`] records.
//!
//! Each parser is a pure function: `(stdout, stderr, unit_dir,
//! workspace_root) -> Vec<Diagnostic>`. No side effects; no executor
//! coupling. They're invoked by the executor's two-pass-on-failure
//! path (ideas-cgj) but kept here as standalone modules so they can
//! be unit-tested against captured fixtures.
//!
//! All file paths in the returned `Diagnostic.file` are normalised
//! to **forward-slash, workspace-relative** so agents can `Read(path)`
//! without further resolution. See [`normalise_path`].

mod cargo_message;
mod eslint;
mod golangci_lint;
mod ruff;

use std::path::Path;

use monad_adapters::{Diagnostic, ParserId};

/// Dispatch captured tool output to the matching parser. Never panics
/// — malformed input returns an empty `Vec`. Parser failures are
/// strictly additive: callers see fewer diagnostics, never an error.
pub fn parse(
    parser: ParserId,
    stdout: &str,
    stderr: &str,
    unit_dir: &Path,
    workspace_root: &Path,
) -> Vec<Diagnostic> {
    match parser {
        ParserId::CargoMessage => cargo_message::parse(stdout, stderr, unit_dir, workspace_root),
        ParserId::GolangciLint => golangci_lint::parse(stdout, stderr, unit_dir, workspace_root),
        ParserId::Eslint => eslint::parse(stdout, stderr, unit_dir, workspace_root),
        ParserId::Ruff => ruff::parse(stdout, stderr, unit_dir, workspace_root),
    }
}

/// Normalise a file path emitted by a tool into the monad convention:
/// forward-slash, relative to the workspace root.
///
/// Tools emit either absolute paths or paths relative to their cwd
/// (typically the unit dir). We handle both:
/// - Absolute path: strip the workspace-root prefix when possible.
/// - Relative path: resolve against `unit_dir` first, then strip
///   the workspace-root prefix.
///
/// When neither strategy yields a path under the workspace root (e.g.
/// a tool reported a vendored path outside the workspace), return the
/// original string unchanged so the diagnostic is still usable.
pub(crate) fn normalise_path(raw: &str, unit_dir: &Path, workspace_root: &Path) -> String {
    let p = Path::new(raw);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        unit_dir.join(p)
    };
    // Strip workspace root prefix when possible.
    let rel = abs.strip_prefix(workspace_root).unwrap_or(abs.as_path());
    // Collapse `./` components — `unit_dir.join("./foo")` keeps the `.`
    // verbatim, which would surface as `apps/api/./foo` to agents.
    let mut clean = std::path::PathBuf::new();
    for c in rel.components() {
        match c {
            std::path::Component::CurDir => {}
            other => clean.push(other.as_os_str()),
        }
    }
    clean.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_returns_empty_for_malformed_input() {
        let unit = PathBuf::from("/repo/apps/api");
        let root = PathBuf::from("/repo");
        for parser in ParserId::all() {
            assert_eq!(
                parse(*parser, "garbage{", "", &unit, &root),
                Vec::<Diagnostic>::new(),
                "{parser:?} should swallow malformed input"
            );
        }
    }

    #[test]
    fn normalise_path_strips_workspace_prefix_for_absolute_paths() {
        let abs = "/repo/apps/api/main.go";
        let unit = PathBuf::from("/repo/apps/api");
        let root = PathBuf::from("/repo");
        assert_eq!(normalise_path(abs, &unit, &root), "apps/api/main.go");
    }

    #[test]
    fn normalise_path_resolves_relative_against_unit_dir() {
        let unit = PathBuf::from("/repo/apps/api");
        let root = PathBuf::from("/repo");
        assert_eq!(normalise_path("main.go", &unit, &root), "apps/api/main.go");
        assert_eq!(
            normalise_path("./cmd/api/main.go", &unit, &root),
            "apps/api/cmd/api/main.go"
        );
    }

    #[test]
    fn normalise_path_returns_original_when_outside_workspace() {
        let unit = PathBuf::from("/repo/apps/api");
        let root = PathBuf::from("/repo");
        // Outside-workspace absolute path stays as-is — agents can
        // still see it, just won't be able to Read() it as a workspace
        // file.
        let outside = "/usr/lib/go/src/runtime/proc.go";
        let out = normalise_path(outside, &unit, &root);
        assert!(out.contains("runtime/proc.go"));
    }

    #[test]
    fn normalise_path_uses_forward_slashes() {
        let unit = PathBuf::from("/repo/apps/api");
        let root = PathBuf::from("/repo");
        let out = normalise_path("src\\nested\\foo.rs", &unit, &root);
        assert!(!out.contains('\\'), "got: {out}");
    }
}
