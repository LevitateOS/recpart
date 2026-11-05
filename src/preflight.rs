use crate::error::{ErrorCode, RecpartError, Result};
use crate::types::{DiskInventory, DiskListResult, DiskTarget, DISK_LIST_SCHEMA_VERSION};
use distro_spec::shared::{is_protected_path, is_root};
use serde::Deserialize;
use std::env;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
struct LsblkListJson {
    #[serde(default)]
    blockdevices: Vec<LsblkDiskRow>,
}

#[derive(Debug, Deserialize)]
struct LsblkDiskRow {
    path: Option<String>,
    #[serde(rename = "type")]
    dev_type: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tran: Option<String>,
    #[serde(default)]
    ro: Option<LsblkReadOnly>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LsblkReadOnly {
    Bool(bool),
    Num(u8),
    Text(String),
}

impl LsblkReadOnly {
    fn is_read_only(&self) -> bool {
        match self {
            LsblkReadOnly::Bool(value) => *value,
            LsblkReadOnly::Num(value) => *value != 0,
            LsblkReadOnly::Text(value) => {
                let normalized = value.trim().to_ascii_lowercase();
                normalized == "1" || normalized == "true" || normalized == "yes"
            }
        }
    }
}

pub const REQUIRED_TOOLS: &[&str] = &[
    "lsblk",
    "sfdisk",
    "wipefs",
    "mkfs.vfat",
    "mkfs.ext4",
    "mount",
    "udevadm",
    "blkid",
];

pub fn tool_in_path(tool: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(tool);
        if !candidate.is_file() {
            return false;
        }

        fs::metadata(candidate)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

pub fn ensure_required_tools() -> Result<()> {
    for tool in REQUIRED_TOOLS {
        if !tool_in_path(tool) {
            return Err(RecpartError::new(
                ErrorCode::MissingTool,
                "preflight",
                format!("required tool '{tool}' exists in PATH"),
                format!("'{tool}' is missing"),
                format!("Install '{tool}' and re-run recpart."),
            ));
        }
    }

    Ok(())
}

pub fn ensure_root_for_apply() -> Result<()> {
    if is_root() {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::NotRoot,
        "preflight",
        "apply runs as root".to_string(),
        "effective uid is not root".to_string(),
        "Re-run with root privileges (sudo).",
    ))
}

pub fn ensure_block_device(path: &Path) -> Result<()> {
    let metadata = fs::metadata(path).map_err(|err| {
        RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "target disk path exists and is a block device",
            format!("failed to stat '{}': {err}", path.display()),
            "Provide a valid block device path (for example /dev/sda).",
        )
    })?;

    if metadata.file_type().is_block_device() {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::InvalidTargetDisk,
        "preflight",
        "target disk path is a block device",
        format!("'{}' is not a block device", path.display()),
        "Use lsblk to choose a disk path like /dev/sdX or /dev/nvme0n1.",
    ))
}

pub fn ensure_mount_root_safe(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(RecpartError::new(
            ErrorCode::MountFailed,
            "preflight",
            "mount root path is absolute",
            format!("'{}' is not absolute", path.display()),
            "Use an absolute mount root path such as /mnt.",
        ));
    }

    if is_protected_path(path) {
        return Err(RecpartError::new(
            ErrorCode::MountFailed,
            "preflight",
            "mount root is not a protected system path",
            format!("'{}' is protected", path.display()),
            "Use a non-protected path such as /mnt or /mnt/recpart.",
        ));
    }

    Ok(())
}

fn resolve_disk_path(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).map_err(|err| {
        RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "target disk path can be canonicalized",
            format!("failed to canonicalize '{}': {err}", path.display()),
            "Use a valid disk path (for example /dev/sda or /dev/nvme0n1).",
        )
    })
}

fn ensure_whole_disk(path: &Path) -> Result<()> {
    let output = Command::new("lsblk")
        .args(["-dn", "-o", "TYPE", &path.to_string_lossy()])
        .output()
        .map_err(|err| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk can query target type",
                format!("failed to execute lsblk: {err}"),
                "Ensure util-linux is installed and lsblk is available.",
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "target type query succeeds",
            stderr.trim().to_string(),
            "Verify target disk path exists and is accessible.",
        ));
    }

    let device_type = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if matches!(device_type.as_str(), "disk" | "loop") {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::InvalidTargetDisk,
        "preflight",
        "target path resolves to whole-disk device type",
        format!("device type is '{}'", device_type),
        "Target a disk device (for example /dev/sda), not a partition like /dev/sda1.",
    ))
}

pub fn ensure_disk_not_mounted(path: &Path) -> Result<()> {
    let output = Command::new("lsblk")
        .args(["-nr", "-o", "MOUNTPOINT", &path.to_string_lossy()])
        .output()
        .map_err(|err| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk can inspect mounted descendants",
                format!("failed to execute lsblk: {err}"),
                "Ensure util-linux is installed and lsblk is available.",
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "mounted-state query succeeds",
            stderr.trim().to_string(),
            "Verify target disk path exists and is accessible.",
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mounted_points = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| *line != "-")
        .map(str::to_string)
        .collect::<Vec<_>>();

    if mounted_points.is_empty() {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::InvalidTargetDisk,
        "preflight",
        "target disk and child partitions are not mounted",
        format!("mounted points detected: {}", mounted_points.join(", ")),
        "Unmount all target disk partitions before destructive apply.",
    ))
}

/// Refuse disks that something else is actively using even when nothing is
/// mounted: device-mapper/LVM/md/dm-crypt holders, active swap, or another
/// exclusive opener. `wipefs` runs without `--force`, so it also refuses busy
/// devices; this check gives a clear error before anything is touched.
pub fn ensure_disk_not_in_use(path: &Path) -> Result<()> {
    let canonical = resolve_disk_path(path)?;
    let disk_name = canonical
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let sys_dir = Path::new("/sys/class/block").join(&disk_name);

    let mut problems = Vec::new();
    let mut device_names = vec![disk_name.clone()];
    device_names.extend(partition_names(&sys_dir));

    for name in &device_names {
        let holders_dir = if *name == disk_name {
            sys_dir.join("holders")
        } else {
            sys_dir.join(name).join("holders")
        };
        if let Ok(entries) = fs::read_dir(&holders_dir) {
            for entry in entries.flatten() {
                problems.push(format!(
                    "/dev/{name} is held by {}",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
    }

    if let Ok(swaps) = fs::read_to_string("/proc/swaps") {
        for line in swaps.lines().skip(1) {
            let Some(swap_dev) = line.split_whitespace().next() else {
                continue;
            };
            let swap_path = fs::canonicalize(swap_dev).unwrap_or_else(|_| PathBuf::from(swap_dev));
            let swap_name = swap_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            if device_names.contains(&swap_name) {
                problems.push(format!("{} is active swap", swap_path.display()));
            }
        }
    }

    if problems.is_empty() {
        if let Err(err) = open_exclusive(&canonical) {
            problems.push(format!(
                "exclusive open of {} failed: {err}",
                canonical.display()
            ));
        }
    }

    if problems.is_empty() {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::InvalidTargetDisk,
        "preflight",
        "target disk and its partitions are not in use (no holders, swap or exclusive openers)",
        problems.join("; "),
        "Deactivate LVM/md/dm-crypt users (vgchange -an, mdadm --stop, cryptsetup close), run swapoff, and close programs using the disk before destructive apply.",
    ))
}

fn partition_names(disk_sys_dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(disk_sys_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().join("partition").exists())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect()
}

/// Sorted names of the partitions the kernel currently exposes for `disk`.
pub fn kernel_partition_names(disk: &Path) -> Vec<String> {
    let name = disk
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut names = partition_names(&Path::new("/sys/class/block").join(name));
    names.sort();
    names
}

fn open_exclusive(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_EXCL)
        .open(path)
        .map(drop)
}

pub fn ensure_disk_writable(path: &Path) -> Result<()> {
    let output = Command::new("lsblk")
        .args(["-dn", "-o", "RO", &path.to_string_lossy()])
        .output()
        .map_err(|err| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk can query read-only status",
                format!("failed to execute lsblk: {err}"),
                "Ensure util-linux is installed and lsblk is available.",
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "disk read-only status query succeeds",
            stderr.trim().to_string(),
            "Verify target disk path exists and is accessible.",
        ));
    }

    let ro = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if ro == "0" {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::InvalidTargetDisk,
        "preflight",
        "target disk is writable (RO=0)",
        format!("read-only flag is RO={ro}"),
        "Use a writable block device target and retry.",
    ))
}

pub fn query_disk_target(path: &Path) -> Result<DiskTarget> {
    let canonical = resolve_disk_path(path)?;
    ensure_block_device(&canonical)?;
    ensure_whole_disk(&canonical)?;

    let output = Command::new("lsblk")
        .args([
            "-b",
            "-dn",
            "-o",
            "SIZE,LOG-SEC,PHY-SEC",
            &canonical.to_string_lossy(),
        ])
        .output()
        .map_err(|err| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk can query disk geometry",
                format!("failed to execute lsblk: {err}"),
                "Ensure util-linux is installed and lsblk is available.",
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "lsblk returns disk geometry",
            stderr.trim().to_string(),
            "Verify the disk path is valid and accessible.",
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.split_whitespace();

    let size_bytes = parts
        .next()
        .ok_or_else(|| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk output contains SIZE",
                stdout.trim().to_string(),
                "Inspect 'lsblk -b -dn -o SIZE,LOG-SEC,PHY-SEC <disk>' output.",
            )
        })?
        .parse::<u64>()
        .map_err(|err| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "SIZE is parseable as integer",
                format!("invalid SIZE value: {err}"),
                "Check lsblk output and locale settings.",
            )
        })?;

    let logical_sector_bytes = parts
        .next()
        .ok_or_else(|| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk output contains LOG-SEC",
                stdout.trim().to_string(),
                "Inspect 'lsblk -b -dn -o SIZE,LOG-SEC,PHY-SEC <disk>' output.",
            )
        })?
        .parse::<u32>()
        .map_err(|err| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "LOG-SEC is parseable as integer",
                format!("invalid LOG-SEC value: {err}"),
                "Check lsblk output and locale settings.",
            )
        })?;

    let physical_sector_bytes = parts
        .next()
        .ok_or_else(|| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk output contains PHY-SEC",
                stdout.trim().to_string(),
                "Inspect 'lsblk -b -dn -o SIZE,LOG-SEC,PHY-SEC <disk>' output.",
            )
        })?
        .parse::<u32>()
        .map_err(|err| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "PHY-SEC is parseable as integer",
                format!("invalid PHY-SEC value: {err}"),
                "Check lsblk output and locale settings.",
            )
        })?;

    Ok(DiskTarget {
        path: canonical,
        size_bytes,
        logical_sector_bytes,
        physical_sector_bytes,
    })
}

pub fn list_candidate_disks() -> Result<Vec<DiskTarget>> {
    Ok(list_disk_inventory()?
        .disks
        .into_iter()
        .map(|disk| DiskTarget {
            path: disk.path,
            size_bytes: disk.size_bytes,
            logical_sector_bytes: disk.logical_sector_bytes,
            physical_sector_bytes: disk.physical_sector_bytes,
        })
        .collect())
}

pub fn list_disk_inventory() -> Result<DiskListResult> {
    let output = Command::new("lsblk")
        .args([
            "-J",
            "-b",
            "-d",
            "-o",
            "PATH,TYPE,SIZE,LOG-SEC,PHY-SEC,MODEL,TRAN,RO",
        ])
        .output()
        .map_err(|err| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk can enumerate block devices",
                format!("failed to execute lsblk: {err}"),
                "Ensure util-linux is installed and lsblk is available.",
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "block device enumeration succeeds",
            stderr.trim().to_string(),
            "Verify the environment has a working lsblk.",
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: LsblkListJson = serde_json::from_str(&stdout).map_err(|err| {
        RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "lsblk JSON output is parseable",
            format!("failed to parse lsblk JSON: {err}"),
            "Inspect 'lsblk -J -b -d -o PATH,TYPE,SIZE,LOG-SEC,PHY-SEC,MODEL,TRAN,RO' output.",
        )
    })?;

    let mut disks = Vec::new();
    for row in parsed.blockdevices {
        let Some(path) = row.path.as_deref() else {
            continue;
        };
        let Some(dev_type) = row.dev_type.as_deref() else {
            continue;
        };
        if !matches!(dev_type, "disk" | "loop") {
            continue;
        }

        if Path::new(path)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("zram"))
        {
            continue;
        }

        let target = match query_disk_target(Path::new(path)) {
            Ok(target) => target,
            Err(err) => {
                eprintln!("recpart: warning: skipping {path}: {err}");
                continue;
            }
        };
        disks.push(DiskInventory {
            path: target.path,
            size_bytes: target.size_bytes,
            logical_sector_bytes: target.logical_sector_bytes,
            physical_sector_bytes: target.physical_sector_bytes,
            model: row
                .model
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("unknown")
                .to_string(),
            transport: row
                .tran
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("unknown")
                .to_string(),
            read_only: row.ro.as_ref().is_some_and(LsblkReadOnly::is_read_only),
        });
    }

    disks.sort_by(|a, b| a.path.cmp(&b.path));
    disks.dedup_by(|a, b| a.path == b.path);
    Ok(DiskListResult {
        schema_version: DISK_LIST_SCHEMA_VERSION,
        disks,
    })
}
