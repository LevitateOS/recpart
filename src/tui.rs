use crate::error::{ErrorCode, RecpartError, Result};
use crate::exec::execute_plan;
use crate::exec::execute_plan_with_reporter;
use crate::plan::build_plan;
use crate::preflight::{ensure_required_tools, list_candidate_disks};
use crate::types::{ApplyOptions, ApplyResult, CommandRecord, InstallMode, PartitionPlan};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;

const MIN_TERMINAL_COLS: u16 = 80;
const MIN_TERMINAL_ROWS: u16 = 24;
const CONFIRM_TOKEN: &str = "DESTROY";

pub fn run_tui() -> Result<()> {
    ensure_required_tools()?;
    enforce_terminal_minimum()?;

    println!("recpart interactive wizard");
    println!();

    let disks = list_candidate_disks()?;
    if disks.is_empty() {
        return Err(RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "tui",
            "at least one candidate disk is available",
            "no disk or loop devices found".to_string(),
            "Attach a target disk and rerun recpart tui.",
        ));
    }

    println!("Available disks:");
    for (i, disk) in disks.iter().enumerate() {
        println!(
            "  {}. {} ({})",
            i + 1,
            disk.path.display(),
            human_size(disk.size_bytes)
        );
    }
    println!();

    let disk_selection = prompt_selection("Select target disk", disks.len(), 1)?;
    let disk = disks[disk_selection - 1].clone();

    println!("\nInstall mode:");
    println!("  1. ab (default): A/B immutable-ready layout");
    println!("  2. mutable: single writable root layout");
    let mode_selection = prompt_selection("Select mode", 2, 1)?;
    let mode = if mode_selection == 2 {
        InstallMode::Mutable
    } else {
        InstallMode::Ab
    };

    let mount_root_input = prompt_line("Mount root [/mnt]: ")?;
    let mount_root = if mount_root_input.trim().is_empty() {
        PathBuf::from("/mnt")
    } else {
        PathBuf::from(mount_root_input.trim())
    };

    let plan = build_plan(disk, mode)?;
    print_plan_summary(&plan);
    println!("\nGenerated sfdisk script:\n");
    println!("{}", plan.sfdisk_script);

    let mut dry_run_result = None;
    if prompt_yes_no("Run dry-run apply first? [Y/n]: ", true)? {
        let result = execute_plan(
            &plan,
            &ApplyOptions {
                dry_run: true,
                confirm_token: None,
                mount_root: mount_root.clone(),
            },
        )?;
        print_dry_run_summary(&result);
        dry_run_result = Some(result);
    }

    println!("\nDestructive apply confirmation");
    println!("  disk: {}", plan.disk.path.display());
    println!("  mode: {}", plan.mode);
    println!("  mount root: {}", mount_root.display());
    println!("  action: wipe partition table, create filesystems, mount target topology");

    if !prompt_yes_no("Proceed with destructive apply now? [y/N]: ", false)? {
        println!("\nNo destructive changes applied.");
        if let Some(result) = dry_run_result {
            print_handoff(&result);
        }
        return Ok(());
    }

    let token = prompt_line(&format!("Type {CONFIRM_TOKEN} to continue: "))?;
    if token.trim() != CONFIRM_TOKEN {
        return Err(RecpartError::new(
            ErrorCode::MissingConfirmation,
            "tui",
            format!("confirmation token '{CONFIRM_TOKEN}' matches exactly"),
            format!("received token '{}'", token.trim()),
            format!("Re-run and type '{CONFIRM_TOKEN}' exactly to proceed."),
        ));
    }

    let total_steps = dry_run_result
        .as_ref()
        .map(|r| r.steps.len())
        .unwrap_or_else(|| {
            execute_plan(
                &plan,
                &ApplyOptions {
                    dry_run: true,
                    confirm_token: None,
                    mount_root: mount_root.clone(),
                },
            )
            .map(|r| r.steps.len())
            .unwrap_or(0)
        });
    let total_steps = if total_steps == 0 { 1 } else { total_steps };

    println!("\nExecution progress:");
    let mut completed = 0usize;
    let mut reporter = |record: &CommandRecord| {
        completed += 1;
        println!(
            "  [{}/{}] [{}] {}",
            completed, total_steps, record.phase, record.command
        );
    };

    let apply_result = execute_plan_with_reporter(
        &plan,
        &ApplyOptions {
            dry_run: false,
            confirm_token: Some(CONFIRM_TOKEN.to_string()),
            mount_root: mount_root.clone(),
        },
        Some(&mut reporter),
    )?;

    println!("\nApply completed successfully.");
    print_handoff(&apply_result);
    Ok(())
}

fn print_plan_summary(plan: &PartitionPlan) {
    println!("\nPlan summary:");
    println!("  Mode: {}", plan.mode);
    println!("  Disk: {}", plan.disk.path.display());
    println!("  Disk size: {}", human_size(plan.disk.size_bytes));
    println!("  Partitions:");
    for p in &plan.partitions {
        let size = p
            .size_mb
            .map(|v| format!("{v}MB"))
            .unwrap_or_else(|| "remaining".to_string());
        println!(
            "    - {}: label={} fs={} mount={} size={}",
            p.index, p.label, p.filesystem, p.mountpoint, size
        );
    }
}

fn print_dry_run_summary(result: &ApplyResult) {
    println!("\nDry-run command sequence:");
    for (i, step) in result.steps.iter().enumerate() {
        println!("  {}. [{}] {}", i + 1, step.phase, step.command);
    }
    println!();
}

fn print_handoff(result: &ApplyResult) {
    println!("\nNext commands:");
    for cmd in &result.handoff.next_commands {
        println!("  {}", cmd);
    }
    println!();
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{}", prompt);
    io::stdout().flush().map_err(|err| {
        RecpartError::new(
            ErrorCode::Internal,
            "tui",
            "stdout flush succeeds",
            err.to_string(),
            "Ensure terminal output is writable.",
        )
    })?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(|err| {
        RecpartError::new(
            ErrorCode::Internal,
            "tui",
            "stdin read succeeds",
            err.to_string(),
            "Ensure terminal input is available.",
        )
    })?;

    Ok(input)
}

fn prompt_selection(label: &str, max: usize, default: usize) -> Result<usize> {
    loop {
        let input = prompt_line(&format!("{} [default {}]: ", label, default))?;
        match parse_selection_input(&input, max, default) {
            Ok(selection) => return Ok(selection),
            Err(err) => {
                println!("{}", err);
            }
        }
    }
}

fn prompt_yes_no(prompt: &str, default_yes: bool) -> Result<bool> {
    loop {
        let input = prompt_line(prompt)?;
        let val = input.trim().to_ascii_lowercase();
        if val.is_empty() {
            return Ok(default_yes);
        }
        if matches!(val.as_str(), "y" | "yes") {
            return Ok(true);
        }
        if matches!(val.as_str(), "n" | "no") {
            return Ok(false);
        }

        println!("Please answer y/yes or n/no.");
    }
}

fn parse_selection_input(
    input: &str,
    max: usize,
    default: usize,
) -> std::result::Result<usize, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(default);
    }

    let parsed = trimmed
        .parse::<usize>()
        .map_err(|_| "Enter a number from the displayed list.".to_string())?;

    if parsed == 0 || parsed > max {
        return Err(format!("Selection must be between 1 and {max}."));
    }

    Ok(parsed)
}

fn enforce_terminal_minimum() -> Result<()> {
    let Some((cols, rows)) = terminal_size() else {
        return Ok(());
    };

    if cols >= MIN_TERMINAL_COLS && rows >= MIN_TERMINAL_ROWS {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::Internal,
        "tui",
        format!(
            "terminal is at least {}x{}",
            MIN_TERMINAL_COLS, MIN_TERMINAL_ROWS
        ),
        format!("detected terminal size {}x{}", cols, rows),
        format!(
            "Resize terminal to at least {} columns and {} rows and retry.",
            MIN_TERMINAL_COLS, MIN_TERMINAL_ROWS
        ),
    ))
}

fn terminal_size_from_env() -> Option<(u16, u16)> {
    let cols = std::env::var("COLUMNS").ok()?.trim().parse::<u16>().ok()?;
    let rows = std::env::var("LINES").ok()?.trim().parse::<u16>().ok()?;
    Some((cols, rows))
}

fn terminal_size_from_stty() -> Option<(u16, u16)> {
    let output = Command::new("stty").arg("size").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.split_whitespace();
    let rows = parts.next()?.parse::<u16>().ok()?;
    let cols = parts.next()?.parse::<u16>().ok()?;
    Some((cols, rows))
}

fn terminal_size() -> Option<(u16, u16)> {
    terminal_size_from_env().or_else(terminal_size_from_stty)
}

#[cfg(test)]
mod tests {
    use super::parse_selection_input;

    #[test]
    fn parse_selection_default_when_empty() {
        assert_eq!(parse_selection_input("  ", 5, 2), Ok(2));
    }

    #[test]
    fn parse_selection_rejects_out_of_range() {
        let err = parse_selection_input("9", 3, 1).expect_err("must fail");
        assert!(err.contains("between 1 and 3"));
    }

    #[test]
    fn parse_selection_accepts_valid_index() {
        assert_eq!(parse_selection_input("3", 3, 1), Ok(3));
    }
}
