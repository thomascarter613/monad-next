//! Human-readable formatter for `monad_core::ExecutionReport`.

use monad_core::{ExecutedProfile, ExecutedUnit, ExecutedTask, ExecutionReport, TaskOutcome};

use crate::style;

pub fn print_human(report: &ExecutionReport) {
    if report.profiles.is_empty() {
        println!("no profiles matched");
        return;
    }

    // Emit GitHub Actions annotations first so they're grouped in the
    // workflow UI even if the rest of the output scrolls past.
    if is_github_actions() {
        emit_gha_annotations(report);
    }

    for (i, monad) in report.profiles.iter().enumerate() {
        if i > 0 {
            println!();
        }
        print_profile(monad);
    }

    println!();
    let s = &report.summary;
    let flaky = if s.flaky > 0 {
        format!(" · {} flaky", s.flaky)
    } else {
        String::new()
    };
    let installs = if s.installs > 0 {
        let fails = if s.install_failures > 0 {
            format!(" ({} failed)", s.install_failures)
        } else {
            String::new()
        };
        format!(" · {} installed{fails}", s.installs)
    } else {
        String::new()
    };
    println!(
        "summary: {} unit{} · {} task{} · {} built · {} cached · {} failed{flaky}{installs} · {}ms",
        s.units,
        if s.units == 1 { "" } else { "es" },
        s.tasks,
        if s.tasks == 1 { "" } else { "s" },
        s.built,
        s.hits,
        s.failed,
        s.duration_ms,
    );
}

fn is_github_actions() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some_and(|v| v == "true")
}

/// Emit one `::error` annotation per failed task (or failed install) so
/// GitHub surfaces the failure in-line on the workflow summary. The
/// `file` attribute points at the unit dir, which is the best we can do
/// generically; future per-adapter lint parsers can upgrade this to
/// file:line locations.
fn emit_gha_annotations(report: &ExecutionReport) {
    for monad in &report.profiles {
        for unit in &monad.units {
            if let Some(install) = &unit.install {
                if let Some(err) = &install.error {
                    let title = format!("{}/install failed", unit.name);
                    println!(
                        "::error file={},title={}::{}",
                        unit.path.display(),
                        escape_gha(&title),
                        escape_gha(err),
                    );
                }
            }
            for task in &unit.tasks {
                if let TaskOutcome::Failed {
                    exit_code,
                    stderr_excerpt,
                } = &task.outcome
                {
                    let title = format!("{}/{} failed", unit.name, task.name);
                    let detail = format_gha_detail(*exit_code, stderr_excerpt);
                    println!(
                        "::error file={},title={}::{}",
                        unit.path.display(),
                        escape_gha(&title),
                        escape_gha(&detail),
                    );
                }
            }
        }
    }
}

fn format_gha_detail(exit_code: i32, stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        format!("exit {exit_code}")
    } else {
        format!("exit {exit_code}: {trimmed}")
    }
}

/// GHA workflow commands treat `%`, `\r`, `\n` specially; escape them per
/// <https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions#example-2>.
fn escape_gha(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn print_profile(monad: &ExecutedProfile) {
    println!(
        "{}: {} ({} unit{})",
        style::bold("monad"),
        style::cyan(&monad.name),
        monad.units.len(),
        if monad.units.len() == 1 { "" } else { "es" }
    );
    for unit in &monad.units {
        println!();
        print_unit(unit);
    }
}

fn print_unit(unit: &ExecutedUnit) {
    let lang = unit
        .language
        .as_deref()
        .map(|l| format!("({l})"))
        .unwrap_or_else(|| "(no adapter)".to_string());
    println!(
        "  {name}  {lang}",
        name = style::bold(&unit.name),
        lang = style::dim(&lang),
    );

    let task_width = unit.tasks.iter().map(|t| t.name.len()).max().unwrap_or(4);
    let name_width = task_width.max("install".len());

    if let Some(install) = &unit.install {
        print_install(install, name_width);
    }

    for task in &unit.tasks {
        print_task(task, name_width);
    }
}

fn print_install(record: &monad_core::InstallRecord, name_width: usize) {
    let (label, detail) = match &record.error {
        None => (style::green("installed "), String::new()),
        Some(e) => (
            style::red("install!  "),
            format!("\n      error: {}", indent_stderr(e)),
        ),
    };
    println!(
        "    {name:<width$}  [{label}]  {reason:<12}  {dur:>5}ms{detail}",
        name = "install",
        width = name_width,
        label = label,
        reason = style::dim(&truncate(&record.reason, 12)),
        dur = record.duration_ms,
        detail = detail,
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

fn print_task(task: &ExecutedTask, name_width: usize) {
    let (label, mut detail) = match &task.outcome {
        TaskOutcome::CacheHit => (style::dim("cache hit "), String::new()),
        TaskOutcome::Built { exit_code: _ } => (style::green("built     "), String::new()),
        TaskOutcome::Failed {
            exit_code,
            stderr_excerpt,
        } => (
            style::red(&format!("failed({exit_code:>2})")),
            if stderr_excerpt.trim().is_empty() {
                String::new()
            } else {
                format!("\n      stderr: {}", indent_stderr(stderr_excerpt))
            },
        ),
        TaskOutcome::Skipped { reason } => (style::yellow("skipped   "), format!(" — {reason}")),
        TaskOutcome::DeployUnchanged {
            last_deployed_at,
            deploy_url,
        } => {
            let url_hint = deploy_url
                .as_deref()
                .map(|u| format!(" → {u}"))
                .unwrap_or_default();
            (
                style::dim("unchanged "),
                format!(" — already deployed {last_deployed_at}{url_hint}"),
            )
        }
    };

    // Integration tasks (deploys, releases, notifications) produce
    // output whose *content* is the result — deploy URLs, release
    // ids, webhook response bodies. Always surface it under the
    // task line, trimmed of trailing whitespace. Monad's executor
    // only populates `output_excerpt` for these; adapter/user tasks
    // (build/test/lint) stay silent on success.
    if let Some(out) = task
        .output_excerpt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        detail.push_str(&format!("\n      output: {}", indent_stderr(out)));
    }

    // Diagnostic count footer — only on failed tasks that produced
    // structured records. Pointer at JSON access; the raw stderr is
    // already above it.
    if !task.diagnostics.is_empty() {
        let n = task.diagnostics.len();
        let plural = if n == 1 { "" } else { "s" };
        detail = format!(
            "{detail}\n      {} {n} diagnostic{plural} captured; pass --json to extract.",
            style::dim("→")
        );
    }

    // Surface flakiness and retry count. Only mention attempts when > 1
    // so the happy path stays terse.
    if task.flaky {
        detail = format!(
            " {}{detail}",
            style::yellow(&format!("(flaky, passed on attempt {})", task.attempts)),
        );
    } else if task.attempts > 1 {
        detail = format!(
            " {}{detail}",
            style::dim(&format!("({} attempts)", task.attempts)),
        );
    }

    let short_key = if task.key.is_empty() {
        String::new()
    } else {
        task.key.chars().take(12).collect::<String>()
    };

    println!(
        "    {name:<width$}  [{label}]  {short}  {dur:>5}ms{detail}",
        name = task.name,
        width = name_width,
        label = label,
        short = style::dim(&format!("{short_key:<12}")),
        dur = task.duration_ms,
        detail = detail,
    );
}

fn indent_stderr(s: &str) -> String {
    s.lines().collect::<Vec<_>>().join("\n              ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_gha_replaces_special_chars() {
        assert_eq!(escape_gha("100% pass"), "100%25 pass");
        assert_eq!(escape_gha("line1\nline2"), "line1%0Aline2");
        assert_eq!(escape_gha("\r\n"), "%0D%0A");
    }

    #[test]
    fn format_gha_detail_trims_stderr_and_falls_back_to_exit() {
        assert_eq!(format_gha_detail(7, "  "), "exit 7");
        assert_eq!(format_gha_detail(1, "boom\n"), "exit 1: boom");
    }
}
