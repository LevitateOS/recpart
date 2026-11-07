//! Planning an explicit layout (`plan --layout`): the actions applied to the
//! disk's inventory give the final table, which is checked against the mode
//! and written as one sfdisk script that keeps every untouched partition's
//! start, size, type, UUID, name and attributes.

use crate::error::{ErrorCode, RecpartError, Result};
use crate::inspect::{align_down, free_gaps, min_shrink_bytes, resize_support, MIB};
use crate::plan::partition_device;
use crate::policy::{
    efi_boot_files_mb, DEFAULT_AB_MIN_STATE_MB, DEFAULT_MUTABLE_MIN_ROOT_MB, MIN_AB_ROOT_MB,
    MIN_NEW_EFI_MB, MIN_SWAP_MB, NOT_MOUNTED,
};
use crate::types::{
    ChangeKind, Filesystem, FreeGap, InspectResult, InstallMode, LayoutAction, LayoutRequest,
    ManualLayout, PartitionChange, PartitionInfo, PartitionPlan, PartitionState, PartitionTemplate,
    ResolvedLayout, Role, SwapFileLocation, SwapFilePlan, TableType, PLAN_SCHEMA_VERSION,
};
use std::collections::{BTreeMap, HashMap};

pub const EFI_TYPE: &str = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B";
pub const LINUX_TYPE: &str = "0FC63DAF-8483-4772-8E79-3D69D8477DE4";
pub const SWAP_TYPE: &str = "0657FD6D-A4AB-43C4-84E5-0933C84B4F4F";
pub const BASIC_DATA_TYPE: &str = "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7";
/// Partitions the install formats or uses start on this boundary at least.
const MIN_ALIGNMENT: u64 = 4096;
/// GPT holds 128 partition entries.
const MAX_PARTITIONS: u32 = 128;

fn refuse(
    code: ErrorCode,
    expectation: impl Into<String>,
    observed: impl Into<String>,
    remediation: impl Into<String>,
) -> RecpartError {
    RecpartError::new(code, "layout", expectation, observed, remediation)
}

/// One partition of the final table.
#[derive(Debug, Clone)]
struct Planned<'a> {
    number: u32,
    start: u64,
    size: u64,
    origin: Option<&'a PartitionInfo>,
    change: ChangeKind,
    /// The filesystem apply makes (created and formatted partitions).
    new_fs: Option<Filesystem>,
    new_label: Option<String>,
    role: Option<Role>,
    mountpoint: Option<String>,
    type_guid: String,
    name: Option<String>,
    uuid: Option<String>,
    attrs: Option<String>,
}

impl Planned<'_> {
    /// The filesystem it has after apply, by blkid name.
    fn filesystem(&self) -> Option<String> {
        self.new_fs
            .map(|fs| fs.as_str().to_string())
            .or_else(|| self.origin.and_then(|o| o.filesystem.clone()))
    }

    fn fs_label(&self) -> Option<String> {
        if self.new_fs.is_some() {
            self.new_label.clone()
        } else {
            self.origin.and_then(|o| o.fs_label.clone())
        }
    }

    fn what(&self) -> String {
        match self.origin {
            Some(o) => format!("partition {} ({})", o.number, o.device),
            None => format!("new partition {}", self.number),
        }
    }
}

/// The role's mount point in the installed system (`None`: not mounted).
pub fn role_mountpoint(role: Role, other: Option<&str>) -> Option<String> {
    match role {
        Role::Efi => Some("/boot".to_string()),
        Role::Root | Role::RootA => Some("/".to_string()),
        Role::RootB | Role::Swap => None,
        Role::State => Some("/var".to_string()),
        Role::Home => Some("/home".to_string()),
        Role::Other => other.map(str::to_string),
    }
}

fn default_filesystem(role: Option<Role>) -> Filesystem {
    match role {
        Some(Role::Efi) => Filesystem::Vfat,
        Some(Role::Swap) => Filesystem::Swap,
        _ => Filesystem::Ext4,
    }
}

fn default_label(role: Option<Role>) -> Option<String> {
    let label = match role? {
        Role::Efi => "EFI",
        Role::Root => "ROOT",
        Role::RootA => "ROOT_A",
        Role::RootB => "ROOT_B",
        Role::State => "STATE",
        Role::Home => "HOME",
        Role::Swap => "SWAP",
        Role::Other => return None,
    };
    Some(label.to_string())
}

fn type_for(fs: Filesystem, role: Option<Role>) -> &'static str {
    match (role, fs) {
        (Some(Role::Efi), _) => EFI_TYPE,
        (_, Filesystem::Swap) => SWAP_TYPE,
        (_, Filesystem::Vfat) => BASIC_DATA_TYPE,
        (_, Filesystem::Ext4) => LINUX_TYPE,
    }
}

/// A mount point the installed system can use: absolute, normalized, not `/`.
fn check_mountpoint(mountpoint: &str) -> Result<()> {
    let ok = mountpoint.starts_with('/')
        && mountpoint.len() > 1
        && !mountpoint.ends_with('/')
        && mountpoint
            .split('/')
            .skip(1)
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains(char::is_whitespace));
    if ok {
        return Ok(());
    }
    Err(refuse(
        ErrorCode::InvalidLayout,
        "an `other` mountpoint is an absolute, normalized path other than /",
        format!("mountpoint {mountpoint:?}"),
        "Give a path such as /srv or /data, without '..', spaces or a trailing slash.",
    ))
}

/// A label the filesystem's mkfs accepts: FAT takes 11 bytes, ext4 and swap 16.
fn check_label(fs: Filesystem, label: &Option<String>) -> Result<()> {
    let Some(label) = label else { return Ok(()) };
    let max = match fs {
        Filesystem::Vfat => 11,
        Filesystem::Ext4 | Filesystem::Swap => 16,
    };
    if label.len() <= max && !label.is_empty() && label.chars().all(|c| c.is_ascii_graphic()) {
        return Ok(());
    }
    Err(refuse(
        ErrorCode::InvalidLayout,
        format!("a {} label is 1 to {max} printable ASCII characters without spaces", fs.as_str()),
        format!("label {label:?}"),
        "Shorten the label, or leave it out for the role's default.",
    ))
}

fn check_role_args(role: Option<Role>, mountpoint: &Option<String>) -> Result<()> {
    match (role, mountpoint) {
        (Some(Role::Other), Some(mp)) => check_mountpoint(mp),
        (Some(Role::Other), None) => Err(refuse(
            ErrorCode::InvalidLayout,
            "role `other` comes with a mountpoint",
            "role other without mountpoint",
            "Add \"mountpoint\": \"/srv\" (or another path) to the action.",
        )),
        (_, Some(mp)) => Err(refuse(
            ErrorCode::InvalidLayout,
            "only role `other` takes a mountpoint",
            format!("mountpoint {mp:?} with role {:?}", role.map(Role::as_str)),
            "Drop the mountpoint, or use role `other`.",
        )),
        _ => Ok(()),
    }
}

pub fn build_manual_plan(
    inventory: &InspectResult,
    mode: InstallMode,
    layout: ManualLayout,
) -> Result<PartitionPlan> {
    let disk = &inventory.disk.target;
    if let Some(expected) = &layout.table_id {
        let found = inventory.table_id.as_deref().unwrap_or("");
        if !expected.eq_ignore_ascii_case(found) {
            return Err(refuse(
                ErrorCode::DiskChanged,
                format!("{} still has partition table {expected}", disk.path.display()),
                format!("table id is {}", inventory.table_id.as_deref().unwrap_or("none")),
                "The disk changed since the layout was made: inspect it again and redo the layout.",
            ));
        }
    }
    if inventory.table == TableType::None && !layout.new_table && !inventory.signatures.is_empty() {
        // Without a table the disk is erased whole; what is on it is no
        // partition the summary would list, so the layout says so itself.
        return Err(refuse(
            ErrorCode::InvalidLayout,
            "a layout for a disk with no partition table but with data on the whole disk sets new_table",
            format!(
                "{} has no partition table, and holds {} on the whole disk",
                disk.path.display(),
                inventory
                    .signatures
                    .iter()
                    .map(|s| s.kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            "Set \"new_table\": true to erase it (the plan's erased_signatures lists what is lost), or pick another disk.",
        ));
    }
    let new_table = layout.new_table || inventory.table == TableType::None;
    if !new_table && inventory.table == TableType::Mbr {
        return Err(refuse(
            ErrorCode::InvalidLayout,
            "recpart edits GPT tables",
            format!("{} has an MBR table", disk.path.display()),
            "Set \"new_table\": true to replace it with GPT (this erases every partition on it), or use the template modes.",
        ));
    }

    // Each existing partition's action, by number.
    let mut by_number: HashMap<u32, &LayoutAction> = HashMap::new();
    for action in &layout.actions {
        let number = match action {
            LayoutAction::Keep { partition, .. }
            | LayoutAction::Delete { partition }
            | LayoutAction::Resize { partition, .. }
            | LayoutAction::Format { partition, .. } => *partition,
            LayoutAction::Create { role, mountpoint, .. } => {
                check_role_args(*role, mountpoint)?;
                continue;
            }
        };
        if !inventory.partitions.iter().any(|p| p.number == number) {
            return Err(refuse(
                ErrorCode::InvalidLayout,
                format!("partition {number} exists on {}", disk.path.display()),
                format!(
                    "partitions are {:?}",
                    inventory.partitions.iter().map(|p| p.number).collect::<Vec<_>>()
                ),
                "Name partitions by the number `recpart inspect` reports.",
            ));
        }
        if by_number.insert(number, action).is_some() {
            return Err(refuse(
                ErrorCode::InvalidLayout,
                "each partition takes at most one action",
                format!("partition {number} has several"),
                "Keep one action per partition (resize and format cannot be combined: delete and create instead).",
            ));
        }
        if new_table && !matches!(action, LayoutAction::Delete { .. }) {
            return Err(refuse(
                ErrorCode::InvalidLayout,
                "a new table erases every partition, so only `create` and `delete` actions follow it",
                format!("partition {number} is kept, resized or formatted with new_table"),
                "Drop new_table to edit the current table, or drop the actions on existing partitions.",
            ));
        }
        match action {
            LayoutAction::Keep { role, mountpoint, .. }
            | LayoutAction::Resize { role, mountpoint, .. }
            | LayoutAction::Format { role, mountpoint, .. } => check_role_args(*role, mountpoint)?,
            _ => {}
        }
    }

    let mut planned: Vec<Planned> = Vec::new();
    let mut erased: Vec<&PartitionInfo> = Vec::new();
    for part in &inventory.partitions {
        let base = Planned {
            number: part.number,
            start: part.start_bytes,
            size: part.size_bytes,
            origin: Some(part),
            change: ChangeKind::Untouched,
            new_fs: None,
            new_label: None,
            role: None,
            mountpoint: None,
            type_guid: part.type_guid.clone(),
            name: part.label.clone(),
            uuid: part.part_uuid.clone(),
            attrs: part.attrs.clone(),
        };
        match by_number.get(&part.number) {
            _ if new_table => erased.push(part),
            Some(LayoutAction::Delete { .. }) => erased.push(part),
            None => planned.push(base),
            Some(LayoutAction::Keep { role, mountpoint, .. }) => planned.push(Planned {
                role: *role,
                mountpoint: mountpoint.clone(),
                ..base
            }),
            Some(LayoutAction::Resize { role, mountpoint, .. }) => planned.push(Planned {
                change: ChangeKind::Resized,
                role: *role,
                mountpoint: mountpoint.clone(),
                ..base
            }),
            Some(LayoutAction::Format {
                filesystem,
                label,
                role,
                mountpoint,
                ..
            }) => {
                let label = label.clone().or_else(|| default_label(*role));
                check_label(*filesystem, &label)?;
                planned.push(Planned {
                    change: ChangeKind::Formatted,
                    new_fs: Some(*filesystem),
                    type_guid: if role.is_some() {
                        type_for(*filesystem, *role).to_string()
                    } else {
                        base.type_guid.clone()
                    },
                    name: if role.is_some() { label.clone() } else { base.name.clone() },
                    new_label: label,
                    role: *role,
                    mountpoint: mountpoint.clone(),
                    ..base
                })
            }
            Some(LayoutAction::Create { .. }) => unreachable!("create names no partition"),
        }
    }

    // Resizes keep the start; a grow takes free space up to the next
    // remaining partition (deleted ones no longer count).
    // A new table is a fresh GPT one, whatever the disk had.
    let (usable_start, usable_end) = if new_table {
        crate::inspect::new_gpt_usable(disk.size_bytes, u64::from(disk.logical_sector_bytes.max(512)))
    } else {
        (inventory.usable_start_bytes, inventory.usable_end_bytes)
    };
    let starts: Vec<u64> = planned.iter().map(|p| p.start).collect();
    for p in planned.iter_mut().filter(|p| p.change == ChangeKind::Resized) {
        let origin = p.origin.expect("resized partitions exist");
        let Some(LayoutAction::Resize { size_mb, .. }) = by_number.get(&origin.number) else {
            unreachable!("resized partitions have a resize action");
        };
        resize_support(origin.filesystem.as_deref()).ok_or_else(|| {
            refuse(
                ErrorCode::ResizeUnsupported,
                "recpart resizes ext4, FAT and NTFS filesystems",
                format!(
                    "{} has filesystem {}",
                    p.what(),
                    origin.filesystem.as_deref().unwrap_or("none")
                ),
                "Keep this partition at its size, or delete it and create a new one (its data is lost).",
            )
        })?;
        if !origin.mountpoints.is_empty() {
            return Err(refuse(
                ErrorCode::ResizeUnsupported,
                "resized filesystems are unmounted (recpart resizes offline)",
                format!("{} is mounted at {}", p.what(), origin.mountpoints.join(", ")),
                format!("Unmount {} and plan again.", origin.device),
            ));
        }
        let limit = starts
            .iter()
            .copied()
            .filter(|s| *s > p.start)
            .min()
            .unwrap_or(usable_end)
            .min(usable_end);
        let new_size = match size_mb {
            Some(0) => {
                return Err(refuse(
                    ErrorCode::InvalidLayout,
                    "size_mb is greater than 0",
                    format!("resize of {} to 0 MiB", p.what()),
                    "Give a positive size, or delete the partition.",
                ))
            }
            Some(mb) => mb * MIB,
            None => align_down(limit, MIB).saturating_sub(p.start).max(p.size),
        };
        if p.start + new_size > limit {
            return Err(refuse(
                ErrorCode::LayoutDoesNotFit,
                format!("{} grows only into the free space after it", p.what()),
                format!(
                    "{} MiB asked, {} MiB available",
                    new_size / MIB,
                    (limit - p.start) / MIB
                ),
                "Grow it less, or delete the partition after it first.",
            ));
        }
        if new_size < p.size {
            let used = origin.used_bytes.ok_or_else(|| {
                refuse(
                    ErrorCode::ShrinkBelowUsed,
                    format!("the used space of {} is known before it shrinks", p.what()),
                    "the used space could not be read",
                    "Run recpart as root so it can read the filesystem, and check it is not damaged.",
                )
            })?;
            let min = min_shrink_bytes(used);
            if new_size < min {
                return Err(refuse(
                    ErrorCode::ShrinkBelowUsed,
                    format!(
                        "{} shrinks to no less than {} MiB (its {} MiB in use plus headroom)",
                        p.what(),
                        min / MIB,
                        used.div_ceil(MIB)
                    ),
                    format!("{} MiB asked", new_size / MIB),
                    "Shrink it less, or free space on it first.",
                ));
            }
        }
        if new_size == p.size {
            p.change = ChangeKind::Untouched;
        }
        p.size = new_size;
    }

    // Created partitions go into the free space left: sized ones first, in
    // order, then those taking the rest of a gap.
    let extents: Vec<(u64, u64)> = planned.iter().map(|p| (p.start, p.start + p.size)).collect();
    let mut gaps: Vec<FreeGap> = free_gaps(usable_start, usable_end, &extents);
    let creates: Vec<&LayoutAction> = layout
        .actions
        .iter()
        .filter(|a| matches!(a, LayoutAction::Create { size_mb: Some(_), .. }))
        .chain(
            layout
                .actions
                .iter()
                .filter(|a| matches!(a, LayoutAction::Create { size_mb: None, .. })),
        )
        .collect();
    for action in creates {
        let LayoutAction::Create {
            size_mb,
            start_mb,
            filesystem,
            label,
            role,
            mountpoint,
        } = action
        else {
            continue;
        };
        if *size_mb == Some(0) {
            return Err(refuse(
                ErrorCode::InvalidLayout,
                "size_mb is greater than 0",
                "create with size_mb 0",
                "Give a positive size, or omit size_mb for the rest of the free space.",
            ));
        }
        let size = size_mb.map(|mb| mb * MIB);
        let what = format!(
            "the new {} partition",
            role.map(Role::as_str).unwrap_or("unassigned")
        );
        let placed = match start_mb {
            Some(start_mb) => {
                let start = start_mb * MIB;
                gaps.iter()
                    .position(|g| g.start_bytes <= start && start < g.start_bytes + g.size_bytes)
                    .and_then(|i| {
                        let end = gaps[i].start_bytes + gaps[i].size_bytes;
                        let size = size.unwrap_or(end - start);
                        (start + size <= end).then_some((i, start, size))
                    })
            }
            None => match size {
                Some(size) => gaps
                    .iter()
                    .position(|g| g.size_bytes >= size)
                    .map(|i| (i, gaps[i].start_bytes, size)),
                None => gaps
                    .iter()
                    .enumerate()
                    .max_by_key(|(i, g)| (g.size_bytes, std::cmp::Reverse(*i)))
                    .map(|(i, g)| (i, g.start_bytes, g.size_bytes)),
            },
        };
        let Some((gap_index, start, size)) = placed else {
            return Err(refuse(
                ErrorCode::LayoutDoesNotFit,
                format!(
                    "{what} ({}) fits in free space{}",
                    size_mb.map(|mb| format!("{mb} MiB")).unwrap_or("rest of a gap".into()),
                    start_mb.map(|s| format!(" at {s} MiB")).unwrap_or_default()
                ),
                format!(
                    "free gaps (MiB at start+size): {}",
                    if gaps.is_empty() {
                        "none".to_string()
                    } else {
                        gaps.iter()
                            .map(|g| format!("{}+{}", g.start_bytes / MIB, g.size_bytes / MIB))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                ),
                "Make it smaller, or delete or shrink a partition to free space.",
            ));
        };
        let gap = gaps.remove(gap_index);
        let gap_end = gap.start_bytes + gap.size_bytes;
        let mut rest = Vec::new();
        if start >= gap.start_bytes + MIB {
            rest.push(FreeGap { start_bytes: gap.start_bytes, size_bytes: start - gap.start_bytes });
        }
        if gap_end >= start + size + MIB {
            rest.push(FreeGap { start_bytes: start + size, size_bytes: gap_end - start - size });
        }
        for (offset, g) in rest.into_iter().enumerate() {
            gaps.insert(gap_index + offset, g);
        }

        let number = (1..=MAX_PARTITIONS)
            .find(|n| !planned.iter().any(|p| p.number == *n))
            .ok_or_else(|| {
                refuse(
                    ErrorCode::LayoutDoesNotFit,
                    format!("the GPT table has a free entry (of {MAX_PARTITIONS})"),
                    "every entry is used",
                    "Delete a partition first.",
                )
            })?;
        let fs = filesystem.unwrap_or_else(|| default_filesystem(*role));
        let label = label.clone().or_else(|| default_label(*role));
        check_label(fs, &label)?;
        planned.push(Planned {
            number,
            start,
            size,
            origin: None,
            change: ChangeKind::Created,
            new_fs: Some(fs),
            new_label: label.clone(),
            role: *role,
            mountpoint: mountpoint.clone(),
            type_guid: type_for(fs, *role).to_string(),
            name: label,
            uuid: None,
            attrs: None,
        });
    }
    planned.sort_by_key(|p| p.number);

    check_roles(&planned, mode, disk.physical_sector_bytes)?;
    let swap_file = plan_swap_file(&layout, &planned, mode)?;

    let mut partitions: Vec<PartitionTemplate> = Vec::new();
    for p in planned.iter().filter(|p| p.role.is_some()) {
        let role = p.role.expect("filtered");
        let filesystem = p
            .filesystem()
            .as_deref()
            .and_then(Filesystem::from_blkid)
            .expect("check_roles accepts only recpart's filesystems for roles");
        partitions.push(PartitionTemplate {
            index: p.number as u8,
            name: role.as_str().to_string(),
            filesystem,
            label: p.fs_label().unwrap_or_default(),
            gpt_type: p.type_guid.clone(),
            bootable: false,
            size_mb: Some(u32::try_from(p.size / MIB).unwrap_or(u32::MAX)),
            mountpoint: role_mountpoint(role, p.mountpoint.as_deref())
                .unwrap_or_else(|| NOT_MOUNTED.to_string()),
        });
    }

    let size_of = |role: Role| -> Option<u32> {
        planned
            .iter()
            .find(|p| p.role == Some(role))
            .map(|p| u32::try_from(p.size / MIB).unwrap_or(u32::MAX))
    };
    let resolved_layout = ResolvedLayout {
        mode,
        efi_size_mb: size_of(Role::Efi).unwrap_or(0),
        root_size_mb: size_of(Role::Root),
        root_a_size_mb: size_of(Role::RootA),
        root_b_size_mb: size_of(Role::RootB),
        state_size_mb: size_of(Role::State),
    };

    let mut summary: Vec<PartitionChange> = erased
        .iter()
        .map(|part| PartitionChange {
            number: part.number,
            device: part.device.clone(),
            change: ChangeKind::Erased,
            before: Some(state_of(part)),
            after: None,
            role: None,
            mountpoint: None,
        })
        .chain(planned.iter().map(|p| PartitionChange {
            number: p.number,
            device: partition_device(&disk.path, p.number),
            change: p.change,
            before: p.origin.map(state_of),
            after: Some(PartitionState {
                start_bytes: p.start,
                size_bytes: p.size,
                filesystem: p.filesystem(),
                fs_label: p.fs_label(),
            }),
            role: p.role,
            mountpoint: p
                .role
                .and_then(|r| role_mountpoint(r, p.mountpoint.as_deref())),
        }))
        .collect();
    summary.sort_by_key(|c| (c.number, c.change != ChangeKind::Erased));

    Ok(PartitionPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        mode,
        layout_request: LayoutRequest::default(),
        resolved_layout,
        disk: disk.clone(),
        partitions,
        sfdisk_script: sfdisk_script(inventory, new_table, &planned),
        manual_layout: Some(layout),
        wipes_disk: new_table,
        summary,
        swap_file,
        disk_serial: inventory.disk.serial.clone(),
        table_id: inventory.table_id.clone(),
        erased_signatures: if new_table {
            inventory.signatures.clone()
        } else {
            Vec::new()
        },
    })
}

pub fn state_of(part: &PartitionInfo) -> PartitionState {
    PartitionState {
        start_bytes: part.start_bytes,
        size_bytes: part.size_bytes,
        filesystem: part.filesystem.clone(),
        fs_label: part.fs_label.clone(),
    }
}

/// The roles make the mode's layout, on filesystems that fit them, at
/// distinct mount points, big enough and aligned.
fn check_roles(planned: &[Planned], mode: InstallMode, physical_sector: u32) -> Result<()> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for p in planned {
        let Some(role) = p.role else { continue };
        *counts.entry(role.as_str()).or_default() += 1;
        let fs = p.filesystem();
        let wanted: &[&str] = match role {
            Role::Efi => &["vfat"],
            Role::Swap => &["swap"],
            Role::Other => &["ext4", "vfat"],
            _ => &["ext4"],
        };
        if !fs.as_deref().is_some_and(|fs| wanted.contains(&fs)) {
            let code = if role == Role::Efi {
                ErrorCode::EfiUnsuitable
            } else {
                ErrorCode::InvalidLayout
            };
            return Err(refuse(
                code,
                format!("a partition with role {} is {}", role.as_str(), wanted.join(" or ")),
                format!("{} is {}", p.what(), fs.as_deref().unwrap_or("unformatted")),
                format!("Format it as {} (its data is lost), or pick another partition.", wanted[0]),
            ));
        }
        let keeps_data = matches!(p.change, ChangeKind::Untouched | ChangeKind::Resized);
        if keeps_data && matches!(role, Role::Root | Role::RootA | Role::RootB | Role::State) {
            return Err(refuse(
                ErrorCode::InvalidLayout,
                format!("the install formats its {} partition", role.as_str()),
                format!("{} keeps its data", p.what()),
                "Give it a `format` action (its data is lost), or create a new partition for the role.",
            ));
        }
        if role == Role::Efi && keeps_data && !p.type_guid.eq_ignore_ascii_case(EFI_TYPE) {
            return Err(refuse(
                ErrorCode::EfiUnsuitable,
                "a reused EFI partition has the EFI system partition type",
                format!("{} has type {}", p.what(), p.type_guid),
                "Pick the disk's EFI system partition, or format this one with role efi.",
            ));
        }
    }

    for p in planned {
        let used = p.role.is_some() || p.change != ChangeKind::Untouched;
        let alignment = MIN_ALIGNMENT.max(u64::from(physical_sector));
        if used && p.start % alignment != 0 {
            return Err(refuse(
                ErrorCode::Misaligned,
                format!("a partition the install changes or uses starts on a {alignment}-byte boundary"),
                format!("{} starts at byte {}", p.what(), p.start),
                "Delete it and create a new partition in its place (new partitions start on 1 MiB boundaries).",
            ));
        }
    }
    let count = |role: Role| counts.get(role.as_str()).copied().unwrap_or(0);
    let (required, forbidden): (&[Role], &[Role]) = match mode {
        InstallMode::Ab => (&[Role::Efi, Role::RootA, Role::RootB, Role::State], &[Role::Root]),
        InstallMode::Mutable => (&[Role::Efi, Role::Root], &[Role::RootA, Role::RootB]),
    };
    for role in required {
        if count(*role) != 1 {
            return Err(refuse(
                ErrorCode::ModeRequirements,
                format!(
                    "a {mode} layout has exactly one partition with role {} (roles needed: {})",
                    role.as_str(),
                    required.iter().map(|r| r.as_str()).collect::<Vec<_>>().join(", ")
                ),
                format!("{} with role {}", count(*role), role.as_str()),
                format!("Give one partition role {}.", role.as_str()),
            ));
        }
    }
    for role in forbidden {
        if count(*role) != 0 {
            return Err(refuse(
                ErrorCode::ModeRequirements,
                format!("a {mode} layout has no partition with role {}", role.as_str()),
                format!("{} with role {}", count(*role), role.as_str()),
                match mode {
                    InstallMode::Ab => "An A/B install uses roles root_a and root_b, not root.",
                    InstallMode::Mutable => "A mutable install uses role root, not root_a/root_b.",
                },
            ));
        }
    }
    if count(Role::State) > 1 {
        return Err(refuse(
            ErrorCode::ModeRequirements,
            "at most one partition has role state",
            format!("{} with role state", count(Role::State)),
            "Give role state to one partition.",
        ));
    }
    let by_role = |role: Role| planned.iter().find(|p| p.role == Some(role));
    if let (Some(a), Some(b)) = (by_role(Role::RootA), by_role(Role::RootB)) {
        if a.size != b.size {
            return Err(refuse(
                ErrorCode::ModeRequirements,
                "the A/B root slots have the same size",
                format!("root_a is {} MiB, root_b is {} MiB", a.size / MIB, b.size / MIB),
                "Give both slots the same size_mb.",
            ));
        }
    }

    let mut seen: HashMap<String, String> = HashMap::new();
    for p in planned {
        let Some(role) = p.role else { continue };
        let Some(mountpoint) = role_mountpoint(role, p.mountpoint.as_deref()) else {
            continue;
        };
        if let Some(first) = seen.insert(mountpoint.clone(), p.what()) {
            return Err(refuse(
                ErrorCode::DuplicateMountpoint,
                format!("one partition mounts at {mountpoint}"),
                format!("{first} and {} both mount there", p.what()),
                "Give each mount point to one partition.",
            ));
        }
    }

    for p in planned {
        let Some(role) = p.role else { continue };
        let changes = p.change != ChangeKind::Untouched;
        let min_mb: u64 = match role {
            Role::Root => u64::from(DEFAULT_MUTABLE_MIN_ROOT_MB),
            Role::RootA | Role::RootB => u64::from(MIN_AB_ROOT_MB),
            Role::State => u64::from(DEFAULT_AB_MIN_STATE_MB),
            Role::Swap => MIN_SWAP_MB,
            _ => 0,
        };
        if changes && p.size < min_mb * MIB {
            return Err(refuse(
                ErrorCode::PartitionTooSmall,
                format!("a {} partition is at least {min_mb} MiB", role.as_str()),
                format!("{} is {} MiB", p.what(), p.size / MIB),
                format!("Give it at least {min_mb} MiB."),
            ));
        }
        if role == Role::Efi {
            let need = efi_boot_files_mb(mode);
            if matches!(p.change, ChangeKind::Created | ChangeKind::Formatted) {
                let min = need.max(MIN_NEW_EFI_MB);
                if p.size < min * MIB {
                    return Err(refuse(
                        ErrorCode::EfiUnsuitable,
                        format!(
                            "a new EFI partition is at least {min} MiB ({need} MiB of kernels and boot loader for {mode})"
                        ),
                        format!("{} is {} MiB", p.what(), p.size / MIB),
                        format!("Give the EFI partition at least {min} MiB."),
                    ));
                }
            } else {
                let origin = p.origin.expect("kept partitions exist");
                let used = origin.used_bytes.ok_or_else(|| {
                    refuse(
                        ErrorCode::EfiUnsuitable,
                        "the free space of the reused EFI partition is known",
                        format!("the used space of {} could not be read", p.what()),
                        "Run recpart as root so it can read the EFI partition.",
                    )
                })?;
                let free = p.size.saturating_sub(used);
                if free < need * MIB {
                    return Err(refuse(
                        ErrorCode::EfiUnsuitable,
                        format!(
                            "the reused EFI partition has {need} MiB free for the {mode} kernels and boot loader"
                        ),
                        format!("{} has {} MiB free", p.what(), free / MIB),
                        "Free space on it, grow it, or create a new EFI partition (use mutable mode for one kernel).",
                    ));
                }
            }
        }
    }

    Ok(())
}

fn plan_swap_file(
    layout: &ManualLayout,
    planned: &[Planned],
    mode: InstallMode,
) -> Result<Option<SwapFilePlan>> {
    let Some(request) = &layout.swap_file else {
        return Ok(None);
    };
    if request.size_mb < MIN_SWAP_MB {
        return Err(refuse(
            ErrorCode::PartitionTooSmall,
            format!("a swap file is at least {MIN_SWAP_MB} MiB"),
            format!("swap_file.size_mb is {}", request.size_mb),
            format!("Give the swap file at least {MIN_SWAP_MB} MiB."),
        ));
    }
    let (role, path) = match (request.location, mode) {
        (SwapFileLocation::Root, InstallMode::Ab) => {
            return Err(refuse(
                ErrorCode::InvalidLayout,
                "an A/B install keeps its swap file on var (the root slots are replaced by updates)",
                "swap_file.location is root",
                "Use \"location\": \"var\".",
            ))
        }
        (SwapFileLocation::Root, InstallMode::Mutable) => (Role::Root, "/swapfile"),
        (SwapFileLocation::Var, _) => (Role::State, "/var/swapfile"),
    };
    let Some(host) = planned.iter().find(|p| p.role == Some(role)) else {
        return Err(refuse(
            ErrorCode::InvalidLayout,
            format!("the swap file's filesystem has a partition with role {}", role.as_str()),
            format!("no partition has role {}", role.as_str()),
            "Give a partition role state for /var, or put the swap file on root (mutable).",
        ));
    };
    if request.size_mb * MIB > host.size / 2 {
        return Err(refuse(
            ErrorCode::LayoutDoesNotFit,
            format!("the swap file takes at most half of its {} partition", role.as_str()),
            format!(
                "{} MiB swap file on a {} MiB partition",
                request.size_mb,
                host.size / MIB
            ),
            "Make the swap file smaller, or its partition larger.",
        ));
    }
    Ok(Some(SwapFilePlan {
        path: path.to_string(),
        size_mb: request.size_mb,
    }))
}

/// The whole table in sfdisk's dump format: untouched partitions are
/// written back with their own start, size, type, UUID, name and attributes.
fn sfdisk_script(inventory: &InspectResult, new_table: bool, planned: &[Planned]) -> String {
    let disk = &inventory.disk.target;
    let sector = u64::from(disk.logical_sector_bytes.max(512));
    let mut lines = vec!["label: gpt".to_string()];
    if !new_table {
        if let Some(id) = &inventory.table_id {
            lines.push(format!("label-id: {id}"));
        }
    }
    lines.push("unit: sectors".to_string());
    if !new_table {
        lines.push(format!("first-lba: {}", inventory.usable_start_bytes / sector));
        lines.push(format!("last-lba: {}", inventory.usable_end_bytes / sector - 1));
    }
    lines.push(format!("sector-size: {sector}"));
    lines.push(String::new());
    for p in planned {
        let mut fields = vec![
            format!("start={}", p.start / sector),
            format!("size={}", p.size / sector),
            format!("type={}", p.type_guid),
        ];
        if let Some(uuid) = &p.uuid {
            fields.push(format!("uuid={uuid}"));
        }
        if let Some(name) = &p.name {
            fields.push(format!("name=\"{}\"", escape(name)));
        }
        if let Some(attrs) = &p.attrs {
            fields.push(format!("attrs=\"{}\"", escape(attrs)));
        }
        lines.push(format!(
            "{} : {}",
            partition_device(&disk.path, p.number),
            fields.join(", ")
        ));
    }
    let mut script = lines.join("\n");
    script.push('\n');
    script
}

/// sfdisk's escaping inside quoted values.
fn escape(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '"' => "\\x22".to_string(),
            '\\' => "\\x5c".to_string(),
            c => c.to_string(),
        })
        .collect()
}
