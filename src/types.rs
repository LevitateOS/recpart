use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

pub const PLAN_SCHEMA_VERSION: u32 = 1;
pub const APPLY_SCHEMA_VERSION: u32 = 2;
pub const HANDOFF_SCHEMA_VERSION: u32 = 2;
pub const ERROR_SCHEMA_VERSION: u32 = 1;
pub const DISK_LIST_SCHEMA_VERSION: u32 = 1;
pub const INSPECT_SCHEMA_VERSION: u32 = 1;

/// The `--confirm` value destructive apply requires.
pub const CONFIRM_TOKEN: &str = "DESTROY";

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum InstallMode {
    /// A/B immutable-ready layout: two root slots for atomic updates and rollback.
    #[default]
    Ab,
    /// Classic layout with one mutable root.
    Mutable,
}

impl fmt::Display for InstallMode {
    /// Its JSON (and CLI) spelling.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).map_err(|_| fmt::Error)?;
        f.write_str(value.as_str().ok_or(fmt::Error)?)
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

/// The filesystems recpart creates. Each serializes as its blkid name.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    Vfat,
    Ext4,
    Swap,
}

impl Filesystem {
    pub const ALL: &'static [Filesystem] = &[Filesystem::Vfat, Filesystem::Ext4, Filesystem::Swap];

    /// The program and arguments that make this filesystem, labelled, on a
    /// device given last (recpart's formatting, and recab's staging of a slot).
    pub fn mkfs(self, label: Option<&str>) -> (&'static str, Vec<String>) {
        let (program, mut args): (&str, Vec<String>) = match self {
            Filesystem::Vfat => ("mkfs.vfat", vec!["-F".into(), "32".into()]),
            Filesystem::Ext4 => ("mkfs.ext4", vec!["-F".into()]),
            Filesystem::Swap => ("mkswap", vec![]),
        };
        if let Some(label) = label {
            args.push(if self == Filesystem::Vfat { "-n" } else { "-L" }.into());
            args.push(label.into());
        }
        (program, args)
    }

    /// The name blkid and lsblk report for it (its serialized name).
    pub fn as_str(self) -> &'static str {
        match self {
            Filesystem::Vfat => "vfat",
            Filesystem::Ext4 => "ext4",
            Filesystem::Swap => "swap",
        }
    }

    pub fn from_blkid(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|fs| fs.as_str() == name)
    }
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionTemplate {
    pub index: u8,
    pub name: String,
    pub filesystem: Filesystem,
    pub label: String,
    pub gpt_type: String,
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
    /// The partitions the installed system uses, with their filesystem and
    /// mount point; a manual layout's other partitions are only in `summary`.
    pub partitions: Vec<PartitionTemplate>,
    pub sfdisk_script: String,
    /// The explicit layout (`--layout`) this plan came from; `None` for the
    /// `ab`/`mutable` templates.
    #[serde(default)]
    pub manual_layout: Option<ManualLayout>,
    /// Apply erases the whole disk (`wipefs -a`) before writing the new
    /// table: always for a template, and for a manual layout with `new_table`.
    #[serde(default)]
    pub wipes_disk: bool,
    /// What happens to every partition, on the disk now and after apply, in
    /// partition-number order: the confirm page's list.
    #[serde(default)]
    pub summary: Vec<PartitionChange>,
    /// The swap file apply creates, if any.
    #[serde(default)]
    pub swap_file: Option<SwapFilePlan>,
    /// The serial of the disk the plan was made for (`inspect`'s
    /// `disk.serial`): with `disk` and `table_id`, it names the disk
    /// `apply --expect` must still find.
    #[serde(default)]
    pub disk_serial: String,
    /// The partition table id the disk had when the plan was made
    /// (`inspect`'s `table_id`); `None` when it had no table, or no id.
    #[serde(default)]
    pub table_id: Option<String>,
    /// What apply's `wipefs -a` erases besides the partitions (`inspect`'s
    /// `signatures`): listed whenever `wipes_disk`, so the confirm page can
    /// say a LUKS container or a filesystem on the bare disk is lost. Empty
    /// when the plan keeps the table.
    #[serde(default)]
    pub erased_signatures: Vec<DiskSignature>,
}

/// Partition sizes the caller asked for; unset sizes take the policy default.
/// The fields are also recpart's `--<field>` CLI flags.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default, clap::Args)]
pub struct LayoutRequest {
    /// EFI partition size in MiB (1024 * 1024 bytes)
    #[arg(long)]
    pub efi_size_mb: Option<u32>,
    /// Mutable root partition size in MiB (mutable mode only)
    #[arg(long)]
    pub root_size_mb: Option<u32>,
    /// A/B slot A partition size in MiB (ab mode only)
    #[arg(long)]
    pub root_a_size_mb: Option<u32>,
    /// A/B slot B partition size in MiB (ab mode only)
    #[arg(long)]
    pub root_b_size_mb: Option<u32>,
    /// A/B state partition size in MiB (ab mode only). Omit to use remaining space.
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
    /// The plan came from a recorded inventory: a dry run skips the live-disk checks.
    pub skip_disk_checks: bool,
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
    /// Swap devices and files apply activated (`swapon`), as recfstab will
    /// find them.
    #[serde(default)]
    pub swaps: Vec<String>,
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
    /// In a dry run, the reasons a real apply would refuse this disk; empty when it would proceed.
    pub warnings: Vec<String>,
    /// The plan's per-partition summary (as in the plan).
    #[serde(default)]
    pub summary: Vec<PartitionChange>,
    /// The dry run planned from a recorded inventory (`--inventory`), so
    /// the live-disk checks behind `warnings` did not run.
    #[serde(default)]
    pub disk_checks_skipped: bool,
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

/// The partition table on a disk.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TableType {
    Gpt,
    /// An MBR (DOS) table.
    Mbr,
    /// No partition table.
    None,
}

/// `recpart inspect`: one disk's partition table, its partitions and the free gaps.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectResult {
    pub schema_version: u32,
    pub disk: DiskInventory,
    pub table: TableType,
    /// The table's identifier (GPT disk GUID, or MBR disk id).
    pub table_id: Option<String>,
    /// The space partitions may use: after the GPT header (or the first MiB)
    /// and before the backup GPT, in bytes from the start of the disk.
    pub usable_start_bytes: u64,
    pub usable_end_bytes: u64,
    /// Ordered by start.
    pub partitions: Vec<PartitionInfo>,
    /// Unpartitioned space of at least 1 MiB, aligned to 1 MiB, ordered by start.
    pub free: Vec<FreeGap>,
    /// Signatures on the whole disk itself, outside any partition: a LUKS
    /// container, a filesystem or an LVM PV written to the bare disk (the
    /// partition table's own `gpt`, `PMBR` and `dos` are not listed). Empty
    /// when there are none, or when neither wipefs nor lsblk could tell.
    #[serde(default)]
    pub signatures: Vec<DiskSignature>,
}

/// A signature on the whole disk, as `wipefs --no-act` reports it.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiskSignature {
    /// wipefs's TYPE: `crypto_LUKS`, `btrfs`, `LVM2_member`, `ext4`, ...
    pub kind: String,
    /// Where the signature's magic is, in bytes from the start of the disk;
    /// 0 when only lsblk's FSTYPE was readable (wipefs needs root).
    pub offset_bytes: u64,
    pub uuid: Option<String>,
    pub label: Option<String>,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionInfo {
    pub number: u32,
    pub device: String,
    pub start_bytes: u64,
    pub size_bytes: u64,
    /// GPT type GUID (upper case), or the MBR type byte (`0x83`).
    pub type_guid: String,
    /// The GPT partition name.
    pub label: Option<String>,
    pub part_uuid: Option<String>,
    /// GPT attributes in sfdisk's notation (`RequiredPartition GUID:63`).
    pub attrs: Option<String>,
    /// The filesystem blkid found (`ext4`, `vfat`, `ntfs`, `swap`, ...).
    pub filesystem: Option<String>,
    pub fs_label: Option<String>,
    pub fs_uuid: Option<String>,
    /// Space in use; `None` when it could not be read (an unmounted
    /// filesystem needs root, or recpart has no probe for it).
    pub used_bytes: Option<u64>,
    /// Where it is mounted now (empty when it is not).
    pub mountpoints: Vec<String>,
    /// The smallest and largest size recpart would resize it to, keeping its
    /// start (the largest takes the free gap right after it); `None` when
    /// recpart cannot resize this filesystem, or cannot read its used space.
    pub min_size_bytes: Option<u64>,
    pub max_size_bytes: Option<u64>,
    /// Why recpart cannot resize it, in words for a person (`XFS can only
    /// grow, never shrink`); `None` when it can.
    #[serde(default)]
    pub resize_blocker: Option<String>,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreeGap {
    pub start_bytes: u64,
    pub size_bytes: u64,
}

/// What the installed system uses a partition for. Each role has its mount
/// point; `other` takes the layout's `mountpoint`.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The EFI system partition, mounted at /boot (vfat).
    Efi,
    /// The one root of a mutable install, at / (ext4).
    Root,
    /// A/B root slot A, installed now and mounted at / (ext4).
    RootA,
    /// A/B root slot B, left unmounted for updates (ext4).
    RootB,
    /// The persistent /var: the A/B state partition, or a mutable install's own /var (ext4).
    State,
    /// /home (ext4).
    Home,
    /// A swap partition, activated with swapon.
    Swap,
    /// Mounted at the action's `mountpoint`.
    Other,
}

impl LayoutAction {
    /// The fields each action takes (`deny_unknown_fields` for an internally
    /// tagged enum, checked by the layout reader).
    pub fn fields(action: &str) -> Option<&'static [&'static str]> {
        Some(match action {
            "keep" => &["action", "partition", "role", "mountpoint"],
            "delete" => &["action", "partition"],
            "create" => &["action", "size_mb", "start_mb", "filesystem", "label", "role", "mountpoint"],
            "resize" => &["action", "partition", "size_mb", "role", "mountpoint"],
            "format" => &["action", "partition", "filesystem", "label", "role", "mountpoint"],
            _ => return None,
        })
    }
}

impl Role {
    /// The template partition name (as `PartitionTemplate::name`).
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Efi => "efi",
            Role::Root => "root",
            Role::RootA => "root_a",
            Role::RootB => "root_b",
            Role::State => "state",
            Role::Home => "home",
            Role::Swap => "swap",
            Role::Other => "other",
        }
    }

    pub fn from_template_name(name: &str) -> Option<Role> {
        [
            Role::Efi,
            Role::Root,
            Role::RootA,
            Role::RootB,
            Role::State,
            Role::Home,
            Role::Swap,
            Role::Other,
        ]
        .into_iter()
        .find(|role| role.as_str() == name)
    }
}

/// An explicit layout: what to do with each partition. Partitions no action
/// names are kept untouched. Sizes are MiB (1024 * 1024 bytes).
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ManualLayout {
    /// Replace the partition table with an empty GPT one, erasing every
    /// partition, before the `create` actions. Only `create` may follow.
    #[serde(default)]
    pub new_table: bool,
    /// When set, the disk's table must still have this id (`inspect`'s
    /// `table_id`), so a plan made for one disk never edits another.
    #[serde(default)]
    pub table_id: Option<String>,
    pub actions: Vec<LayoutAction>,
    /// A swap file in the installed system, in place of (or as well as) a
    /// swap partition.
    #[serde(default)]
    pub swap_file: Option<SwapFileRequest>,
}

/// One action of a manual layout. `partition` is an existing partition's
/// number from `inspect`; each partition takes at most one action. `role`
/// and `mountpoint` (for `other`) say how the installed system uses it.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum LayoutAction {
    /// Leave the partition and its data as they are (an existing EFI
    /// partition, /home, or another system's partition).
    Keep {
        partition: u32,
        #[serde(default)]
        role: Option<Role>,
        #[serde(default)]
        mountpoint: Option<String>,
    },
    /// Remove the partition; its data is lost.
    Delete { partition: u32 },
    /// A new partition in free space, formatted. `size_mb` unset takes the
    /// rest of the free gap; `start_mb` picks the gap (first fit when unset).
    /// `filesystem` defaults from the role (ext4 without one).
    Create {
        #[serde(default)]
        size_mb: Option<u64>,
        #[serde(default)]
        start_mb: Option<u64>,
        #[serde(default)]
        filesystem: Option<Filesystem>,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        role: Option<Role>,
        #[serde(default)]
        mountpoint: Option<String>,
    },
    /// Grow or shrink the partition together with its filesystem, keeping
    /// its start and data. `size_mb` unset grows it into the free gap after it.
    Resize {
        partition: u32,
        #[serde(default)]
        size_mb: Option<u64>,
        #[serde(default)]
        role: Option<Role>,
        #[serde(default)]
        mountpoint: Option<String>,
    },
    /// Make a new filesystem on the partition, in place; its data is lost.
    Format {
        partition: u32,
        filesystem: Filesystem,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        role: Option<Role>,
        #[serde(default)]
        mountpoint: Option<String>,
    },
}

/// Where a swap file goes: the root or the /var filesystem.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwapFileLocation {
    Root,
    Var,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapFileRequest {
    pub size_mb: u64,
    pub location: SwapFileLocation,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SwapFilePlan {
    /// The file's path in the installed system (`/swapfile`, `/var/swapfile`).
    pub path: String,
    pub size_mb: u64,
}

/// What apply does to one partition.
#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    /// Deleted (or lost with the old table); its data is gone.
    Erased,
    /// New, formatted.
    Created,
    /// Grown or shrunk with its filesystem; the data stays.
    Resized,
    /// A new filesystem in place; its data is gone.
    Formatted,
    /// Left as it is.
    Untouched,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionState {
    pub start_bytes: u64,
    pub size_bytes: u64,
    pub filesystem: Option<String>,
    pub fs_label: Option<String>,
}

#[cfg_attr(feature = "contracts", derive(schemars::JsonSchema, ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartitionChange {
    pub number: u32,
    pub device: String,
    pub change: ChangeKind,
    /// The partition on the disk now (`None` when it is created).
    pub before: Option<PartitionState>,
    /// The partition after apply (`None` when it is erased).
    pub after: Option<PartitionState>,
    pub role: Option<Role>,
    /// Where the installed system mounts it (`None`: not mounted).
    pub mountpoint: Option<String>,
}
