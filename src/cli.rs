use crate::error::Result;
use crate::exec::execute_plan;
use crate::json::to_pretty_json;
use crate::plan::build_plan_with_layout_request;
use crate::preflight::query_disk_target;
use crate::tui::run_tui;
use crate::types::{ApplyOptions, InstallMode, LayoutRequest};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "recpart")]
#[command(version)]
#[command(about = "Mode-aware partition planning and apply backend")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Generate a deterministic partition plan.
    Plan(PlanArgs),
    /// Apply a partition plan directly to disk.
    Apply(ApplyArgs),
    /// Reserved frontend/TUI entrypoint.
    Tui,
}

#[derive(Debug, Clone, Parser)]
pub struct PlanArgs {
    /// Target disk block device (for example /dev/sda)
    #[arg(long)]
    pub disk: PathBuf,

    /// Install mode: ab (default) or mutable
    #[arg(long, value_enum, default_value_t = ModeArg::Ab)]
    pub mode: ModeArg,

    /// Emit machine-readable JSON
    #[arg(long)]
    pub json: bool,

    #[command(flatten)]
    pub layout: LayoutRequestArgs,
}

#[derive(Debug, Clone, Parser)]
pub struct ApplyArgs {
    /// Target disk block device (for example /dev/sda)
    #[arg(long)]
    pub disk: PathBuf,

    /// Install mode: ab (default) or mutable
    #[arg(long, value_enum, default_value_t = ModeArg::Ab)]
    pub mode: ModeArg,

    /// Mount root used for target topology (default /mnt)
    #[arg(long, default_value = "/mnt")]
    pub mount_root: PathBuf,

    /// Plan and print commands without touching disk state.
    #[arg(long)]
    pub dry_run: bool,

    /// Emit machine-readable JSON
    #[arg(long)]
    pub json: bool,

    /// Destructive apply confirmation token (must be DESTROY)
    #[arg(long)]
    pub confirm: Option<String>,

    #[command(flatten)]
    pub layout: LayoutRequestArgs,
}

#[derive(Debug, Clone, Default, Args)]
pub struct LayoutRequestArgs {
    /// EFI partition size in MB
    #[arg(long)]
    pub efi_size_mb: Option<u32>,

    /// Mutable root partition size in MB (mutable mode only)
    #[arg(long)]
    pub root_size_mb: Option<u32>,

    /// A/B slot A partition size in MB (ab mode only)
    #[arg(long)]
    pub root_a_size_mb: Option<u32>,

    /// A/B slot B partition size in MB (ab mode only)
    #[arg(long)]
    pub root_b_size_mb: Option<u32>,

    /// A/B state partition size in MB (ab mode only). Omit to use remaining space.
    #[arg(long)]
    pub state_size_mb: Option<u32>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ModeArg {
    Ab,
    Mutable,
}

impl From<ModeArg> for InstallMode {
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Ab => InstallMode::Ab,
            ModeArg::Mutable => InstallMode::Mutable,
        }
    }
}

impl Cli {
    pub fn json_requested(&self) -> bool {
        match &self.command {
            Some(Commands::Plan(args)) => args.json,
            Some(Commands::Apply(args)) => args.json,
            Some(Commands::Tui) | None => false,
        }
    }
}

impl LayoutRequestArgs {
    fn to_layout_request(&self) -> LayoutRequest {
        LayoutRequest {
            efi_size_mb: self.efi_size_mb,
            root_size_mb: self.root_size_mb,
            root_a_size_mb: self.root_a_size_mb,
            root_b_size_mb: self.root_b_size_mb,
            state_size_mb: self.state_size_mb,
        }
    }
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Commands::Plan(args)) => run_plan(args),
        Some(Commands::Apply(args)) => run_apply(args),
        Some(Commands::Tui) | None => run_tui(),
    }
}

fn run_plan(args: PlanArgs) -> Result<()> {
    let disk = query_disk_target(&args.disk)?;
    let mode: InstallMode = args.mode.into();
    let plan = build_plan_with_layout_request(disk, mode, args.layout.to_layout_request())?;

    if args.json {
        println!("{}", to_pretty_json(&plan)?);
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

fn run_apply(args: ApplyArgs) -> Result<()> {
    let disk = query_disk_target(&args.disk)?;
    let mode: InstallMode = args.mode.into();
    let plan = build_plan_with_layout_request(disk, mode, args.layout.to_layout_request())?;

    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: args.dry_run,
            confirm_token: args.confirm,
            mount_root: args.mount_root,
        },
    )?;

    if args.json {
        println!("{}", to_pretty_json(&result)?);
    } else {
        println!("recpart apply");
        println!("  mode: {}", result.mode);
        println!("  disk: {}", result.disk.path.display());
        println!("  dry-run: {}", result.dry_run);
        println!("  steps: {}", result.steps.len());

        println!("\nHandoff commands:");
        for cmd in &result.handoff.next_commands {
            println!("  {}", cmd);
        }
    }

    Ok(())
}
