//! `monad artifacts` — list resolved output paths per unit.
//!
//! Data layer lives in `monad_core::artifacts` so `monad-mcp` can
//! reuse it. This module keeps only the CLI-side printing.

use std::collections::BTreeMap;

use anyhow::Result;
use monad_config::Workspace;

use crate::cli::GlobalFlags;
use crate::style;

pub fn run(global: &GlobalFlags) -> Result<i32> {
    let root = crate::resolve_workspace_root(global)?;
    let workspace = Workspace::load(&root)?;
    let by_unit = monad_core::artifacts::collect(&workspace, global.monad.as_deref())?;

    if global.json {
        let payload: BTreeMap<&String, Vec<String>> = by_unit
            .iter()
            .map(|(name, paths)| {
                (
                    name,
                    paths.iter().map(|p| p.display().to_string()).collect(),
                )
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if by_unit.is_empty() {
        println!(
            "{} no resolved artefacts — check that your units declare [outputs] \
             and that you've built them",
            style::yellow("note:")
        );
    } else {
        for (unit, paths) in &by_unit {
            println!("{}", style::cyan(unit));
            for p in paths {
                println!("  {}", p.display());
            }
        }
    }
    Ok(0)
}
