use crate::error::Result;
use crate::exec::execute_plan;
use crate::plan::build_plan;
use crate::preflight::{list_disk_inventory, query_disk_target};
use crate::types::{ApplyOptions, InstallMode, LayoutRequest};
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
    /// Generate a deterministic partition plan.
    Plan(PlanArgs),
    /// Apply a partition plan directly to disk.
    Apply(ApplyArgs),
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

fn run_plan(args: PlanArgs, json: bool) -> Result<()> {
    let plan = plan_for(args)?;

    if json {
        println!("{}", to_json(&plan));
    } else {
        println!("recpart plan");
        println!("  mode: {}", plan.mode);
        println!("  disk: {}", plan.disk.path.display());
        println!("  size: {} bytes", plan.disk.size_bytes);
        println!("\nGenerated sfdisk script:\n");
        print!("{}", plan.sfdisk_script);
    }

    Ok(())
}

fn plan_for(args: PlanArgs) -> Result<crate::types::PartitionPlan> {
    build_plan(query_disk_target(&args.disk)?, args.mode, args.layout)
}

fn run_apply(args: ApplyArgs, json: bool) -> Result<()> {
    let plan = plan_for(args.plan)?;

    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: args.dry_run,
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
