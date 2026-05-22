//! Workspace inventory data for `monad unit list` + `monad box list`.
//!
//! Pure read: `Workspace` in, inventory out. Shared by the CLI's
//! list verbs and by the `monad-mcp` tools — both call the same
//! functions, serialise to the same JSON shape.

use schemars::JsonSchema;
use serde::Serialize;

use monad_config::Workspace;

use crate::scan_orphan_units;

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UnitListOutput {
    pub units: Vec<UnitListItem>,
    /// Workspace-relative paths of `unit.toml` files on disk that
    /// aren't wired into any monad. Agents should surface these so
    /// the user can `monad unit add <path>` — or they'll be invisible
    /// to `monad plan`.
    pub orphans: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct UnitListItem {
    /// Unit name from unit.toml.
    pub name: String,
    /// Workspace-relative path to the unit directory.
    pub path: String,
    /// Language id (e.g. "bun", "cargo"), if declared.
    pub language: Option<String>,
    /// Names of profiles that include this unit.
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BoxListOutput {
    pub profiles: Vec<BoxListItem>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BoxListItem {
    pub name: String,
    /// Workspace-relative path to the monad source file
    /// (e.g. `profiles/prod.toml`).
    pub source: String,
    /// Unit paths this monad includes, verbatim from `units = [...]`.
    pub units: Vec<String>,
}

/// Build the unit list for a loaded workspace.
pub fn unit_list(workspace: &Workspace) -> UnitListOutput {
    let mut units: Vec<UnitListItem> = workspace
        .units_by_name
        .values()
        .map(|d| {
            let rel = d.rel.to_string_lossy().to_string();
            let profiles = workspace
                .profiles
                .values()
                .filter(|b| b.config.units.iter().any(|dp| dp == &rel))
                .map(|b| b.config.name.clone())
                .collect();
            UnitListItem {
                name: d.config.name.clone(),
                path: rel,
                language: d.config.language.clone(),
                profiles,
            }
        })
        .collect();
    units.sort_by(|a, b| a.name.cmp(&b.name));

    let orphans = scan_orphan_units(workspace)
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    UnitListOutput { units, orphans }
}

/// Build the box (monad) list for a loaded workspace.
pub fn box_list(workspace: &Workspace) -> BoxListOutput {
    let mut profiles: Vec<BoxListItem> = workspace
        .profiles
        .values()
        .map(|b| BoxListItem {
            name: b.config.name.clone(),
            source: b
                .source
                .strip_prefix(&workspace.root)
                .unwrap_or(&b.source)
                .to_string_lossy()
                .to_string(),
            units: b.config.units.clone(),
        })
        .collect();
    profiles.sort_by(|a, b| a.name.cmp(&b.name));

    BoxListOutput { profiles }
}
