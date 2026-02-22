use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

pub const PLAN_SCHEMA_VERSION: u32 = 1;
pub const APPLY_SCHEMA_VERSION: u32 = 1;
pub const HANDOFF_SCHEMA_VERSION: u32 = 1;
pub const ERROR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskTarget {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub logical_sector_bytes: u32,
    pub physical_sector_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionTemplate {
    pub index: u8,
    pub name: String,
    pub filesystem: String,
    pub label: String,
    pub gpt_type: String,
    pub size_mb: Option<u32>,
    pub mountpoint: String,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LayoutRequest {
    pub efi_size_mb: Option<u32>,
    pub root_size_mb: Option<u32>,
    pub root_a_size_mb: Option<u32>,
    pub root_b_size_mb: Option<u32>,
    pub state_size_mb: Option<u32>,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MountMapping {
    pub path: String,
    pub device: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeContext {
    pub install_target_slot: Option<String>,
    pub inactive_slot_hint: Option<String>,
    pub slot_a_device: Option<String>,
    pub slot_b_device: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffPayload {
    pub schema_version: u32,
    pub mode: InstallMode,
    pub install_target: String,
    pub mount_map: Vec<MountMapping>,
    pub next_commands: Vec<String>,
    pub mode_context: ModeContext,
}

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
    pub mounted: Vec<MountMapping>,
    pub handoff: HandoffPayload,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorPayload {
    pub schema_version: u32,
    pub code: String,
    pub component: String,
    pub expectation: String,
    pub observed: String,
    pub remediation: String,
}
