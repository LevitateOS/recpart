use crate::error::{ErrorCode, RecpartError, Result};
use crate::exec::execute_plan;
use crate::inspect::{inspect_disk, load_inventory, MIB};
use crate::manual::build_manual_plan;
use crate::plan::build_plan;
use crate::preflight::list_disk_inventory;
use crate::types::{ApplyOptions, InspectResult, InstallMode, LayoutRequest, ManualLayout};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "recpart")]
#[command(version)]
#[command(about = "Mode-aware partition planning and apply backend")]
pub struct Cli {
    /// Emit machine-readable JSON (errors too, on stderr)
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Enumerate candidate target disks.
    ListDisks,
    /// Show a disk's partition table, partitions (filesystem, used space,
    /// mounts) and free gaps.
    Inspect(InspectArgs),
    /// Generate a deterministic partition plan.
    Plan(PlanArgs),
    /// Apply a partition plan directly to disk.
    Apply(ApplyArgs),
}

#[derive(Debug, Clone, Parser)]
pub struct InspectArgs {
    /// Disk block device (for example /dev/sda)
    #[arg(long)]
    pub disk: PathBuf,

    /// Show a recorded `inspect --json` document of that disk instead of
    /// reading it (free gaps and resize bounds are recomputed).
    #[arg(long, value_name = "FILE")]
    pub inventory: Option<PathBuf>,
}

#[derive(Debug, Clone, Parser)]
pub struct PlanArgs {
    /// Target disk block device (for example /dev/sda)
    #[arg(long)]
    pub disk: PathBuf,

    /// Install mode
    #[arg(long, value_enum, default_value_t = InstallMode::Ab)]
    pub mode: InstallMode,

    #[command(flatten)]
    pub layout: LayoutRequest,

    /// An explicit layout (JSON: keep, delete, create, resize and format
    /// actions with roles) in place of the mode's template; `-` reads stdin.
    #[arg(
        long = "layout",
        value_name = "FILE",
        conflicts_with_all = ["efi_size_mb", "root_size_mb", "root_a_size_mb", "root_b_size_mb", "state_size_mb"]
    )]
    pub layout_file: Option<PathBuf>,

    /// Plan against a recorded `inspect --json` document instead of reading
    /// the disk (plan and dry-run only).
    #[arg(long, value_name = "FILE")]
    pub inventory: Option<PathBuf>,
}

#[derive(Debug, Clone, Parser)]
pub struct ApplyArgs {
    #[command(flatten)]
    pub plan: PlanArgs,

    /// Mount root used for target topology (default /mnt)
    #[arg(long, default_value = "/mnt")]
    pub mount_root: PathBuf,

    /// Plan and print commands without touching disk state.
    #[arg(long)]
    pub dry_run: bool,

    /// Destructive apply confirmation token (must be DESTROY)
    #[arg(long)]
    pub confirm: Option<String>,
}

/// recpart's JSON output; every value it prints is plain data.
pub fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).expect("recpart output types serialize to JSON")
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::ListDisks => run_list_disks(cli.json),
        Commands::Inspect(args) => run_inspect(args, cli.json),
        Commands::Plan(args) => run_plan(args, cli.json),
        Commands::Apply(args) => run_apply(args, cli.json),
    }
}

fn run_list_disks(json: bool) -> Result<()> {
    let inventory = list_disk_inventory()?;

    if json {
        println!("{}", to_json(&inventory));
    } else {
        println!("recpart candidate disks");
        for disk in &inventory.disks {
            println!("  {}", disk.target.path.display());
            println!(
                "    size: {} bytes (logical {} / physical {})",
                disk.target.size_bytes, disk.target.logical_sector_bytes, disk.target.physical_sector_bytes
            );
            println!(
                "    model: {} | transport: {} | read-only: {}",
                disk.model,
                disk.transport,
                if disk.read_only { "yes" } else { "no" }
            );
        }
    }

    Ok(())
}

fn run_inspect(args: InspectArgs, json: bool) -> Result<()> {
    let inventory = inventory_of(&args.disk, args.inventory.as_deref())?;
    if json {
        println!("{}", to_json(&inventory));
        return Ok(());
    }
    let mib = |bytes: u64| bytes / MIB;
    println!(
        "{}: {} MiB, {:?} table{}",
        inventory.disk.target.path.display(),
        mib(inventory.disk.target.size_bytes),
        inventory.table,
        inventory.table_id.as_deref().map(|id| format!(" {id}")).unwrap_or_default()
    );
    for part in &inventory.partitions {
        println!(
            "  {:>3} {:<16} start {:>8} MiB  size {:>8} MiB  {:<6} {:<12} used {:<10} {}",
            part.number,
            part.device,
            mib(part.start_bytes),
            mib(part.size_bytes),
            part.filesystem.as_deref().unwrap_or("-"),
            part.fs_label.as_deref().or(part.label.as_deref()).unwrap_or("-"),
            part.used_bytes.map(|u| format!("{} MiB", u.div_ceil(MIB))).unwrap_or("?".into()),
            part.mountpoints.join(",")
        );
    }
    for gap in &inventory.free {
        println!(
            "      free             start {:>8} MiB  size {:>8} MiB",
            mib(gap.start_bytes),
            mib(gap.size_bytes)
        );
    }
    Ok(())
}

fn run_plan(args: PlanArgs, json: bool) -> Result<()> {
    let plan = plan_for(&args)?;

    if json {
        println!("{}", to_json(&plan));
    } else {
        println!("recpart plan");
        println!("  mode: {}", plan.mode);
        println!("  disk: {}", plan.disk.path.display());
        println!("  size: {} bytes", plan.disk.size_bytes);
        println!("\nPartitions:");
        for change in &plan.summary {
            println!(
                "  {:>3} {:<16} {:<9} {}",
                change.number,
                change.device,
                format!("{:?}", change.change).to_lowercase(),
                change
                    .role
                    .map(|r| format!("{} {}", r.as_str(), change.mountpoint.as_deref().unwrap_or("")))
                    .unwrap_or_default()
            );
        }
        if let Some(file) = &plan.swap_file {
            println!("  swap file {} ({} MiB)", file.path, file.size_mb);
        }
        println!("\nGenerated sfdisk script:\n");
        print!("{}", plan.sfdisk_script);
    }

    Ok(())
}

fn invalid_layout(path: &std::path::Path, observed: String) -> RecpartError {
    RecpartError::new(
        ErrorCode::InvalidLayout,
        "layout",
        format!("'{}' is a recpart layout (JSON)", path.display()),
        observed,
        "See the layout format in recpart's README.",
    )
}

fn read_layout(path: &std::path::Path) -> Result<ManualLayout> {
    let text = if path.as_os_str() == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
            .map_err(|err| invalid_layout(path, err.to_string()))?;
        text
    } else {
        std::fs::read_to_string(path).map_err(|err| invalid_layout(path, err.to_string()))?
    };
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|err| invalid_layout(path, err.to_string()))?;
    let actions = value.get("actions").and_then(|a| a.as_array());
    for (index, action) in actions.into_iter().flatten().enumerate() {
        let (Some(name), Some(fields)) = (
            action.get("action").and_then(|a| a.as_str()),
            action.as_object(),
        ) else {
            continue;
        };
        let Some(allowed) = crate::types::LayoutAction::fields(name) else {
            continue;
        };
        if let Some(unknown) = fields.keys().find(|k| !allowed.contains(&k.as_str())) {
            return Err(invalid_layout(
                path,
                format!("actions[{index}] ({name}) has unknown field `{unknown}`; it takes {}", allowed.join(", ")),
            ));
        }
    }
    serde_json::from_value(value).map_err(|err| invalid_layout(path, err.to_string()))
}

fn inventory_of(disk: &std::path::Path, recorded: Option<&std::path::Path>) -> Result<InspectResult> {
    let Some(path) = recorded else {
        return inspect_disk(disk);
    };
    let inventory = load_inventory(path)?;
    if inventory.disk.target.path != disk {
        return Err(RecpartError::new(
            ErrorCode::InvalidInventory,
            "inspect",
            format!("the inventory describes {}", disk.display()),
            format!("'{}' describes {}", path.display(), inventory.disk.target.path.display()),
            "Pass the --disk the inventory was recorded for.",
        ));
    }
    Ok(inventory)
}

fn plan_for(args: &PlanArgs) -> Result<crate::types::PartitionPlan> {
    let inventory = inventory_of(&args.disk, args.inventory.as_deref())?;
    match &args.layout_file {
        Some(path) => build_manual_plan(&inventory, args.mode, read_layout(path)?),
        None => build_plan(&inventory, args.mode, args.layout.clone()),
    }
}

fn run_apply(args: ApplyArgs, json: bool) -> Result<()> {
    if args.plan.inventory.is_some() && !args.dry_run {
        return Err(RecpartError::new(
            ErrorCode::InvalidInventory,
            "apply",
            "a recorded inventory is only planned and dry-run",
            "--inventory without --dry-run",
            "Drop --inventory so apply reads the disk itself, or add --dry-run.",
        ));
    }
    let plan = plan_for(&args.plan)?;

    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: args.dry_run,
            skip_disk_checks: args.plan.inventory.is_some(),
            confirm_token: args.confirm,
            mount_root: args.mount_root,
        },
    )?;

    if json {
        println!("{}", to_json(&result));
    } else {
        println!("recpart apply");
        println!("  mode: {}", result.mode);
        println!("  disk: {}", result.disk.path.display());
        println!("  dry-run: {}", result.dry_run);
        println!("\nSteps:");
        for step in &result.steps {
            println!("  [{}] {}", step.phase, step.command);
        }
        if result.disk_checks_skipped {
            println!("\nPlanned from a recorded inventory: the live-disk checks did not run.");
        }
        if !result.warnings.is_empty() {
            println!("\nApply would refuse this disk:");
            for warning in &result.warnings {
                println!("  {warning}");
            }
        }

        println!("\nInstall target: {}", result.handoff.install_target);
        for mount in &result.handoff.mount_map {
            println!("  {} <- {}", mount.path, mount.device);
        }
    }

    Ok(())
}
