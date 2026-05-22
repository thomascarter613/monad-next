//! Human-readable formatter for [`monad_core::Plan`].
//!
//! TTY-aware colours via [`crate::style`]; structure matches `run_print`
//! so humans reading a `plan → ci` flow see a consistent layout.

use monad_core::{Plan, PlannedUnit, PlannedTask, TaskStatus};

use crate::style;

pub fn print_human(plan: &Plan) {
    if plan.profiles.is_empty() {
        println!("no profiles to plan");
        return;
    }

    for (i, monad) in plan.profiles.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!(
            "{}: {} monad ({} unit{})",
            style::bold("plan"),
            style::cyan(&monad.name),
            monad.units.len(),
            if monad.units.len() == 1 { "" } else { "es" }
        );

        for unit in &monad.units {
            println!();
            print_unit(unit);
        }
    }

    println!();
    let s = &plan.summary;
    println!(
        "summary: {} unit{} · {} task{} · {} miss · {} hit{}",
        s.units,
        if s.units == 1 { "" } else { "es" },
        s.tasks,
        if s.tasks == 1 { "" } else { "s" },
        s.misses,
        s.hits,
        if s.skipped > 0 {
            format!(" · {} skipped (diff-clean)", s.skipped)
        } else {
            String::new()
        },
    );
}

fn print_unit(unit: &PlannedUnit) {
    let language = unit
        .language
        .as_deref()
        .map(|l| format!("({l})"))
        .unwrap_or_else(|| "(no adapter)".to_string());

    println!(
        "  {name}  {lang}",
        name = style::bold(&unit.name),
        lang = style::dim(&language),
    );

    if unit.skipped_by_diff {
        println!("    diff-clean — no tasks to run");
        return;
    }

    if unit.tasks.is_empty() {
        println!("    (no tasks)");
        return;
    }

    let name_width = unit.tasks.iter().map(|t| t.name.len()).max().unwrap_or(4);

    for task in &unit.tasks {
        print_task(task, name_width);
    }
}

fn print_task(task: &PlannedTask, name_width: usize) {
    let status = match task.status {
        TaskStatus::CacheHit => style::dim("cache hit "),
        TaskStatus::CacheMiss => style::yellow("cache miss"),
        TaskStatus::NoAdapter => style::dim("no adapter"),
        TaskStatus::SkippedDiffClean => style::dim("skipped   "),
    };
    let short = if task.key.is_empty() {
        String::new()
    } else {
        task.key.chars().take(12).collect::<String>()
    };
    println!(
        "    {name:<width$}  [{status}]  {short}",
        name = task.name,
        width = name_width,
        status = status,
        short = style::dim(&short),
    );
}
