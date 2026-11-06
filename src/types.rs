use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

pub const PLAN_SCHEMA_VERSION: u32 = 1;
pub const APPLY_SCHEMA_VERSION: u32 = 2;
pub const HANDOFF_SCHEMA_VERSION: u32 = 2;
pub const ERROR_SCHEMA_VERSION: u32 = 1;
pub const DISK_LIST_SCHEMA_VERSION: u32 = 1;

/// The `--confirm` value destructive apply requires.
pub const CONFIRM_TOKEN: &str = "DESTROY";

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum InstallMode {
    #[default]
    Ab,
    Mutable,
}

impl fmt::Display for InstallMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallMode::Ab => write!(f, "ab"),
            InstallMode::Mutable => write!(f, "mutable"),
        }
    }
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskTarget {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub logical_sector_bytes: u32,
    pub physical_sector_bytes: u32,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskInventory {
    #[serde(flatten)]
    pub target: DiskTarget,
    pub model: String,
    pub vendor: String,
    pub serial: String,
    pub transport: String,
    /// Spinning disk; `false` for SSDs and virtual disks.
    pub rotational: bool,
    pub removable: bool,
    pub read_only: bool,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskListResult {
    pub schema_version: u32,
    pub disks: Vec<DiskInventory>,
}

/// The filesystems recpart creates.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    Vfat,
    Ext4,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionTemplate {
    pub index: u8,
    pub name: String,
    pub filesystem: Filesystem,
    pub label: String,
    pub gpt_type: String,
    /// The legacy-BIOS bootable flag (set on the ESP).
    pub bootable: bool,
    pub size_mb: Option<u32>,
    pub mountpoint: String,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionPlan {
    pub schema_version: u32,
    pub mode: InstallMode,
    pub layout_request: LayoutRequest,
    pub resolved_layout: ResolvedLayout,
    pub disk: DiskTarget,
    pub partitions: Vec<PartitionTemplate>,
    pub sfdisk_script: String,
}

/// Partition sizes the caller asked for; unset sizes take the policy default.
/// The fields are also recpart's `--<field>` CLI flags.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, clap::Args)]
pub struct LayoutRequest {
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

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedLayout {
    pub mode: InstallMode,
    pub efi_size_mb: u32,
    pub root_size_mb: Option<u32>,
    pub root_a_size_mb: Option<u32>,
    pub root_b_size_mb: Option<u32>,
    pub state_size_mb: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub dry_run: bool,
    pub confirm_token: Option<String>,
    pub mount_root: PathBuf,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountMapping {
    pub path: String,
    pub device: String,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// A/B facts of the layout (all `None` in mutable mode): which slot the
/// install target is ("A"/"B"), the other slot, and each slot's partition.
pub struct ModeContext {
    pub install_target_slot: Option<String>,
    pub inactive_slot_hint: Option<String>,
    pub slot_a_device: Option<String>,
    pub slot_b_device: Option<String>,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffPayload {
    pub schema_version: u32,
    pub mode: InstallMode,
    pub install_target: String,
    pub mount_map: Vec<MountMapping>,
    pub mode_context: ModeContext,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRecord {
    pub phase: String,
    pub command: String,
    pub program: String,
    pub args: Vec<String>,
    pub status: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub dry_run: bool,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyResult {
    pub schema_version: u32,
    pub mode: InstallMode,
    pub layout_request: LayoutRequest,
    pub resolved_layout: ResolvedLayout,
    pub disk: DiskTarget,
    pub dry_run: bool,
    pub steps: Vec<CommandRecord>,
    pub partition_map: Vec<PartitionTemplate>,
    pub formatted_devices: Vec<String>,
    pub handoff: HandoffPayload,
    pub warnings: Vec<String>,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub schema_version: u32,
    pub code: String,
    pub component: String,
    pub expectation: String,
    pub observed: String,
    pub remediation: String,
}
