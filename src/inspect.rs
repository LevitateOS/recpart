//! `recpart inspect`: what is on a disk now. The table and its partitions
//! come from `sfdisk --json` when the disk is readable (root), otherwise from
//! lsblk's udev view; filesystems, labels, UUIDs and mounts come from lsblk.
//! The used space of an unmounted filesystem is read with that filesystem's
//! own tool (dumpe2fs, fsck.fat -n, ntfsresize --info), which needs root.

use crate::error::{ErrorCode, RecpartError, Result};
use crate::plan::partition_device;
use crate::preflight::{lsblk, query_disk};
use crate::types::{
    DiskInventory, FreeGap, InspectResult, PartitionInfo, TableType, INSPECT_SCHEMA_VERSION,
};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MIB: u64 = 1024 * 1024;

/// The columns `inspect` reads for the disk and each partition.
const PART_COLUMNS: &str =
    "PATH,TYPE,START,SIZE,PARTN,PARTTYPE,PARTLABEL,PARTUUID,PARTFLAGS,FSTYPE,LABEL,UUID,FSSIZE,FSUSED,MOUNTPOINTS,PTTYPE,PTUUID";

/// GPT keeps 128 entries of 128 bytes, plus a header sector, at each end.
const GPT_ENTRIES_BYTES: u64 = 128 * 128;

#[derive(Debug, Deserialize)]
struct LsblkList {
    #[serde(default)]
    blockdevices: Vec<LsblkPart>,
}

#[derive(Debug, Deserialize)]
struct LsblkPart {
    path: String,
    #[serde(rename = "type")]
    dev_type: String,
    start: Option<u64>,
    size: Option<u64>,
    partn: Option<u32>,
    parttype: Option<String>,
    partlabel: Option<String>,
    partuuid: Option<String>,
    partflags: Option<String>,
    fstype: Option<String>,
    label: Option<String>,
    uuid: Option<String>,
    fssize: Option<u64>,
    fsused: Option<u64>,
    #[serde(default)]
    mountpoints: Vec<Option<String>>,
    pttype: Option<String>,
    ptuuid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SfdiskJson {
    partitiontable: SfdiskTable,
}

#[derive(Debug, Deserialize)]
struct SfdiskTable {
    label: String,
    id: Option<String>,
    firstlba: Option<u64>,
    lastlba: Option<u64>,
    #[serde(default)]
    partitions: Vec<SfdiskPart>,
}

#[derive(Debug, Deserialize)]
struct SfdiskPart {
    node: String,
    start: u64,
    size: u64,
    #[serde(rename = "type")]
    part_type: String,
    uuid: Option<String>,
    name: Option<String>,
    attrs: Option<String>,
}

/// The inventory of the disk at `path`.
pub fn inspect_disk(path: &Path) -> Result<InspectResult> {
    let disk = query_disk(path)?;
    let device = disk.target.path.to_string_lossy().to_string();
    let stdout = lsblk(
        &["-J", "-b", "-l", "-o", PART_COLUMNS, &device],
        "describe the disk's partitions",
    )?;
    let listed: LsblkList = serde_json::from_str(&stdout).map_err(|err| {
        RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "inspect",
            "lsblk JSON output is parseable",
            format!("failed to parse lsblk JSON: {err}"),
            format!("Inspect 'lsblk -J -b -l -o {PART_COLUMNS} {device}' output."),
        )
    })?;
    let whole = listed.blockdevices.iter().find(|d| d.path == device);
    let parts: Vec<&LsblkPart> = listed
        .blockdevices
        .iter()
        .filter(|d| d.dev_type == "part")
        .collect();
    let sfdisk = read_sfdisk(&disk.target.path);

    let sector = u64::from(disk.target.logical_sector_bytes.max(512));
    let table = match sfdisk
        .as_ref()
        .map(|t| t.label.as_str())
        .or(whole.and_then(|d| d.pttype.as_deref()))
    {
        Some("gpt") => TableType::Gpt,
        Some("dos") => TableType::Mbr,
        _ => TableType::None,
    };
    let table_id = sfdisk
        .as_ref()
        .and_then(|t| t.id.clone())
        .or_else(|| whole.and_then(|d| d.ptuuid.clone()))
        .map(|id| id.to_uppercase());

    let (default_start, default_end) = new_gpt_usable(disk.target.size_bytes, sector);
    let (usable_start_bytes, usable_end_bytes) = match &sfdisk {
        Some(t) => (
            t.firstlba.map(|lba| lba * sector).unwrap_or(default_start),
            t.lastlba.map(|lba| (lba + 1) * sector).unwrap_or(default_end),
        ),
        None if table == TableType::Mbr => (MIB, disk.target.size_bytes),
        None => (default_start, default_end),
    };

    let mut partitions: Vec<PartitionInfo> = match &sfdisk {
        Some(t) => t
            .partitions
            .iter()
            .filter_map(|p| {
                let number = trailing_number(&p.node)?;
                let fs = parts.iter().find(|l| l.partn == Some(number));
                Some(partition_info(
                    &disk,
                    number,
                    p.start * sector,
                    p.size * sector,
                    p.part_type.clone(),
                    p.name.clone(),
                    p.uuid.clone(),
                    p.attrs.clone(),
                    fs.copied(),
                ))
            })
            .collect(),
        None => parts
            .iter()
            .filter_map(|l| {
                Some(partition_info(
                    &disk,
                    l.partn?,
                    // lsblk's START counts 512-byte sectors, whatever the disk's sector size.
                    l.start? * 512,
                    l.size?,
                    l.parttype.clone().unwrap_or_default(),
                    l.partlabel.clone(),
                    l.partuuid.clone(),
                    l.partflags.as_deref().and_then(gpt_attrs_from_flags),
                    Some(l),
                ))
            })
            .collect(),
    };
    partitions.sort_by_key(|p| p.start_bytes);
    Ok(finish_inventory(
        disk,
        table,
        table_id,
        usable_start_bytes,
        usable_end_bytes,
        partitions,
    ))
}

/// Free gaps and resize bounds, from the partitions: shared by `inspect` and
/// by a layout planned over a recorded inventory.
pub fn finish_inventory(
    disk: DiskInventory,
    table: TableType,
    table_id: Option<String>,
    usable_start_bytes: u64,
    usable_end_bytes: u64,
    mut partitions: Vec<PartitionInfo>,
) -> InspectResult {
    let extents: Vec<(u64, u64)> = partitions
        .iter()
        .map(|p| (p.start_bytes, p.start_bytes + p.size_bytes))
        .collect();
    let free = free_gaps(usable_start_bytes, usable_end_bytes, &extents);
    for part in &mut partitions {
        let end = part.start_bytes + part.size_bytes;
        let next = extents
            .iter()
            .map(|(start, _)| *start)
            .filter(|start| *start >= end)
            .min()
            .unwrap_or(usable_end_bytes)
            .min(usable_end_bytes);
        let resizable = resize_support(part.filesystem.as_deref()).is_some() && part.mountpoints.is_empty();
        part.max_size_bytes = resizable.then(|| align_down(next, MIB).max(end) - part.start_bytes);
        part.min_size_bytes = if resizable {
            part.used_bytes.map(|used| min_shrink_bytes(used).min(part.size_bytes))
        } else {
            None
        };
    }
    InspectResult {
        schema_version: INSPECT_SCHEMA_VERSION,
        disk,
        table,
        table_id,
        usable_start_bytes,
        usable_end_bytes,
        partitions,
        free,
    }
}

/// A recorded `inspect` document (a fixture, or one saved earlier).
pub fn load_inventory(path: &Path) -> Result<InspectResult> {
    let fail = |observed: String| {
        RecpartError::new(
            ErrorCode::InvalidInventory,
            "inspect",
            format!("'{}' is a recpart inspect document", path.display()),
            observed,
            "Record one with 'recpart inspect --disk <disk> --json > inventory.json'.",
        )
    };
    let text = fs::read_to_string(path).map_err(|err| fail(err.to_string()))?;
    let inventory: InspectResult = serde_json::from_str(&text).map_err(|err| fail(err.to_string()))?;
    if inventory.schema_version != INSPECT_SCHEMA_VERSION {
        return Err(fail(format!(
            "schema_version {}, expected {INSPECT_SCHEMA_VERSION}",
            inventory.schema_version
        )));
    }
    let mut partitions = inventory.partitions;
    partitions.sort_by_key(|p| p.start_bytes);
    Ok(finish_inventory(
        inventory.disk,
        inventory.table,
        inventory.table_id,
        inventory.usable_start_bytes,
        inventory.usable_end_bytes,
        partitions,
    ))
}

/// The space a new GPT table leaves for partitions, aligned to 1 MiB.
pub fn new_gpt_usable(size_bytes: u64, sector: u64) -> (u64, u64) {
    let table_sectors = GPT_ENTRIES_BYTES.div_ceil(sector) + 1;
    let end = size_bytes.saturating_sub(table_sectors * sector);
    (MIB, end)
}

/// Unpartitioned runs of at least 1 MiB inside the usable space, aligned to 1 MiB.
pub fn free_gaps(usable_start: u64, usable_end: u64, extents: &[(u64, u64)]) -> Vec<FreeGap> {
    let mut sorted = extents.to_vec();
    sorted.sort();
    let mut gaps = Vec::new();
    let mut cursor = usable_start;
    for (start, end) in sorted.iter().copied().chain([(usable_end, usable_end)]) {
        let gap_start = align_up(cursor, MIB);
        let gap_end = align_down(start.min(usable_end), MIB);
        if gap_end >= gap_start + MIB {
            gaps.push(FreeGap {
                start_bytes: gap_start,
                size_bytes: gap_end - gap_start,
            });
        }
        cursor = cursor.max(end);
    }
    gaps
}

pub fn align_up(value: u64, to: u64) -> u64 {
    value.div_ceil(to) * to
}

pub fn align_down(value: u64, to: u64) -> u64 {
    value / to * to
}

/// The smallest size recpart shrinks a filesystem with `used` bytes to:
/// the used space plus 10% and 64 MiB of headroom, in whole MiB.
pub fn min_shrink_bytes(used: u64) -> u64 {
    align_up(used + used / 10 + 64 * MIB, MIB)
}

/// How recpart resizes a filesystem, by its blkid name: `None` when it
/// cannot resize it safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeTool {
    /// e2fsck -f, then resize2fs (offline).
    Ext,
    /// fatresize (it also rewrites the partition's entry; recpart's table write follows).
    Fat,
    /// ntfsresize (it refuses a hibernated or unclean volume).
    Ntfs,
}

pub fn resize_support(filesystem: Option<&str>) -> Option<ResizeTool> {
    match filesystem? {
        "ext4" | "ext3" | "ext2" => Some(ResizeTool::Ext),
        "vfat" => Some(ResizeTool::Fat),
        "ntfs" => Some(ResizeTool::Ntfs),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn partition_info(
    disk: &DiskInventory,
    number: u32,
    start_bytes: u64,
    size_bytes: u64,
    type_guid: String,
    label: Option<String>,
    part_uuid: Option<String>,
    attrs: Option<String>,
    fs: Option<&LsblkPart>,
) -> PartitionInfo {
    let device = partition_device(&disk.target.path, number);
    let nonempty = |v: Option<String>| v.filter(|v| !v.trim().is_empty());
    let filesystem = nonempty(fs.and_then(|f| f.fstype.clone()));
    let mountpoints: Vec<String> = fs
        .map(|f| f.mountpoints.iter().flatten().cloned().collect())
        .unwrap_or_default();
    let used_bytes = match fs.and_then(|f| f.fssize.zip(f.fsused)) {
        // Mounted: the kernel's own count (lsblk reports it only for mounted filesystems).
        Some((fs_size, fs_used)) if !mountpoints.is_empty() => {
            Some(size_bytes.saturating_sub(fs_size.saturating_sub(fs_used)))
        }
        _ => probe_free_bytes(filesystem.as_deref(), &device).map(|free| size_bytes.saturating_sub(free)),
    };
    let type_guid = if type_guid.len() > 4 {
        type_guid.to_uppercase()
    } else {
        type_guid
    };
    PartitionInfo {
        number,
        device,
        start_bytes,
        size_bytes,
        type_guid,
        label: nonempty(label),
        part_uuid: part_uuid.map(|u| u.to_uppercase()),
        attrs: nonempty(attrs),
        filesystem,
        fs_label: nonempty(fs.and_then(|f| f.label.clone())),
        fs_uuid: nonempty(fs.and_then(|f| f.uuid.clone())),
        used_bytes,
        mountpoints,
        min_size_bytes: None,
        max_size_bytes: None,
    }
}

fn read_sfdisk(disk: &Path) -> Option<SfdiskTable> {
    let output = Command::new("sfdisk").arg("--json").arg(disk).output().ok()?;
    if !output.status.success() {
        return None;
    }
    serde_json::from_slice::<SfdiskJson>(&output.stdout)
        .ok()
        .map(|j| j.partitiontable)
}

fn trailing_number(node: &str) -> Option<u32> {
    let digits: String = node
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

/// lsblk's PARTFLAGS (GPT attribute bits, hex) in sfdisk's `attrs` notation.
fn gpt_attrs_from_flags(flags: &str) -> Option<String> {
    let bits = u64::from_str_radix(flags.trim().trim_start_matches("0x"), 16).ok()?;
    let mut names = Vec::new();
    for (bit, name) in [(0, "RequiredPartition"), (1, "NoBlockIOProtocol"), (2, "LegacyBIOSBootable")] {
        if bits & (1 << bit) != 0 {
            names.push(name.to_string());
        }
    }
    for bit in 48..64 {
        if bits & (1u64 << bit) != 0 {
            names.push(format!("GUID:{bit}"));
        }
    }
    (!names.is_empty()).then(|| names.join(" "))
}

/// Free bytes of an unmounted filesystem, read with its own tool; `None`
/// when the device is unreadable (not root) or there is no probe for it.
fn probe_free_bytes(filesystem: Option<&str>, device: &str) -> Option<u64> {
    let run = |program: &str, args: &[&str]| -> Option<String> {
        let output = Command::new(program).args(args).arg(device).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    };
    match filesystem? {
        "ext4" | "ext3" | "ext2" => {
            let text = run("dumpe2fs", &["-h"])?;
            let field = |name: &str| -> Option<u64> {
                text.lines()
                    .find_map(|l| l.strip_prefix(name))
                    .and_then(|rest| rest.trim_start_matches(':').trim().parse().ok())
            };
            Some(field("Free blocks")? * field("Block size")?)
        }
        "vfat" => {
            // `fsck.fat -n` changes nothing; -v prints the cluster size.
            let text = run("fsck.fat", &["-n", "-v"])?;
            let cluster: u64 = text
                .lines()
                .find(|l| l.trim_end().ends_with("bytes per cluster"))?
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            let counts = text.lines().rev().find(|l| l.contains(" clusters"))?;
            let fraction = counts.split_whitespace().find(|w| w.contains('/'))?;
            let (used, total) = fraction.split_once('/')?;
            let used: u64 = used.parse().ok()?;
            let total: u64 = total.parse().ok()?;
            Some(total.saturating_sub(used) * cluster)
        }
        "ntfs" => {
            // "You might resize at 9575895040 bytes or 9576 MB (freeing 15957 MB)."
            let text = run("ntfsresize", &["--info", "--no-progress-bar"])?;
            let min: u64 = text
                .lines()
                .find_map(|l| l.split("resize at ").nth(1))?
                .split_whitespace()
                .next()?
                .parse()
                .ok()?;
            let size = block_device_size(device)?;
            Some(size.saturating_sub(min))
        }
        "swap" => block_device_size(device),
        _ => None,
    }
}

fn block_device_size(device: &str) -> Option<u64> {
    let name = PathBuf::from(device).file_name()?.to_string_lossy().to_string();
    let sectors: u64 = fs::read_to_string(format!("/sys/class/block/{name}/size"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(sectors * 512)
}
