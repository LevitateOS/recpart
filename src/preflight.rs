use crate::error::{ErrorCode, RecpartError, Result};
use crate::types::{DiskInventory, DiskListResult, DiskTarget, DISK_LIST_SCHEMA_VERSION};
use serde::Deserialize;
use std::env;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use target_root::{is_protected_path, is_root};

/// Where the kernel lists block devices and their partitions and holders.
const SYS_BLOCK: &str = "/sys/class/block";

/// The columns recpart reads for a disk, in one `lsblk -J` query.
const DISK_COLUMNS: &str = "PATH,TYPE,SIZE,LOG-SEC,PHY-SEC,MODEL,VENDOR,SERIAL,TRAN,ROTA,RM,RO";

#[derive(Debug, Deserialize)]
struct LsblkJson {
    #[serde(default)]
    blockdevices: Vec<LsblkDisk>,
}

#[derive(Debug, Deserialize)]
struct LsblkDisk {
    path: PathBuf,
    #[serde(rename = "type")]
    dev_type: String,
    size: u64,
    #[serde(rename = "log-sec")]
    log_sec: u32,
    #[serde(rename = "phy-sec")]
    phy_sec: u32,
    model: Option<String>,
    vendor: Option<String>,
    serial: Option<String>,
    tran: Option<String>,
    rota: LsblkFlag,
    rm: LsblkFlag,
    ro: LsblkFlag,
}

/// lsblk prints flags as JSON booleans (util-linux >= 2.37) or as "0"/"1".
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LsblkFlag {
    Bool(bool),
    Text(String),
}

impl LsblkFlag {
    fn is_set(&self) -> bool {
        match self {
            LsblkFlag::Bool(value) => *value,
            LsblkFlag::Text(value) => value.trim() == "1",
        }
    }
}

impl LsblkDisk {
    fn is_whole_disk(&self) -> bool {
        matches!(self.dev_type.as_str(), "disk" | "loop")
    }

    fn into_inventory(self) -> DiskInventory {
        let known = |value: Option<String>| {
            value
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "unknown".to_string())
        };
        DiskInventory {
            read_only: self.ro.is_set(),
            rotational: self.rota.is_set(),
            removable: self.rm.is_set(),
            model: known(self.model),
            vendor: known(self.vendor),
            serial: known(self.serial),
            transport: known(self.tran),
            target: DiskTarget {
                path: self.path,
                size_bytes: self.size,
                logical_sector_bytes: self.log_sec,
                physical_sector_bytes: self.phy_sec,
            },
        }
    }
}

/// Run lsblk and return its stdout; `what` names the query in errors.
pub(crate) fn lsblk(args: &[&str], what: &str) -> Result<String> {
    let fail = |observed: String| {
        RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            format!("lsblk can {what}"),
            observed,
            "Check that the device exists and util-linux's lsblk is installed.",
        )
    };
    let output = Command::new("lsblk")
        .args(args)
        .output()
        .map_err(|err| fail(format!("failed to execute lsblk: {err}")))?;
    if !output.status.success() {
        return Err(fail(String::from_utf8_lossy(&output.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The disks lsblk reports, without their partitions; `device` narrows the
/// query to one device.
fn lsblk_disks(device: Option<&Path>) -> Result<Vec<LsblkDisk>> {
    let mut args = vec!["-J", "-b", "-d", "-o", DISK_COLUMNS];
    let device = device.map(|d| d.to_string_lossy().into_owned());
    if let Some(device) = &device {
        args.push(device);
    }
    let stdout = lsblk(&args, "describe block devices")?;
    let parsed: LsblkJson = serde_json::from_str(&stdout).map_err(|err| {
        RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "lsblk JSON output is parseable",
            format!("failed to parse lsblk JSON: {err}"),
            format!("Inspect 'lsblk -J -b -d -o {DISK_COLUMNS}' output."),
        )
    })?;
    Ok(parsed.blockdevices)
}

pub(crate) fn tool_in_path(tool: &str) -> bool {
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

/// Every program apply will run is on PATH.
pub fn ensure_required_tools(programs: &[String]) -> Result<()> {
    for tool in programs {
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
        "Re-run as root.",
    ))
}

fn ensure_block_device(path: &Path) -> Result<()> {
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
            ErrorCode::InvalidMountRoot,
            "preflight",
            "mount root path is absolute",
            format!("'{}' is not absolute", path.display()),
            "Use an absolute mount root path such as /mnt.",
        ));
    }

    if is_protected_path(path) {
        return Err(RecpartError::new(
            ErrorCode::InvalidMountRoot,
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

pub fn ensure_disk_not_mounted(path: &Path) -> Result<()> {
    let stdout = lsblk(
        &["-nr", "-o", "MOUNTPOINT", &path.to_string_lossy()],
        "list mounts on the disk and its partitions",
    )?;
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
    let unreadable = |what: String, err: std::io::Error| {
        RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "recpart can tell whether the target disk is in use",
            format!("cannot read {what}: {err}"),
            "Run recpart apply as root on a system with /sys and /proc mounted.",
        )
    };
    let disk_name = canonical
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .ok_or_else(|| unreadable(canonical.display().to_string(), std::io::Error::other("it names no device")))?;
    let sys_dir = Path::new(SYS_BLOCK).join(&disk_name);

    let mut problems = Vec::new();
    let mut devices = vec![(disk_name, sys_dir.clone())];
    devices.extend(
        partition_names(&sys_dir)?
            .into_iter()
            .map(|name| (name.clone(), sys_dir.join(name))),
    );
    let device_names: Vec<&String> = devices.iter().map(|(name, _)| name).collect();

    for (name, dir) in &devices {
        // A device without a holders directory has no holders.
        let holders = target_root::dir_names(&dir.join("holders"))
            .map_err(|err| unreadable(format!("/dev/{name}'s holders"), err))?;
        for holder in holders {
            problems.push(format!("/dev/{name} is held by {}", holder.to_string_lossy()));
        }
    }

    let swaps = fs::read_to_string("/proc/swaps").map_err(|err| unreadable("/proc/swaps".to_string(), err))?;
    for line in swaps.lines().skip(1) {
        let Some(swap_dev) = line.split_whitespace().next() else {
            continue;
        };
        // /proc/swaps escapes spaces in a swap file's name as \040.
        let swap_dev = PathBuf::from(std::ffi::OsString::from(
            String::from_utf8_lossy(&target_root::unescape_octal(swap_dev)).into_owned(),
        ));
        let swap_path = match fs::canonicalize(&swap_dev) {
            Ok(path) => path,
            // A swap file removed while in use is still listed, with " (deleted)".
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(unreadable(format!("swap '{}'", swap_dev.display()), err)),
        };
        let swap_name = swap_path.file_name().map(|name| name.to_string_lossy().to_string());
        if swap_name.is_some_and(|name| device_names.contains(&&name)) {
            problems.push(format!("{} is active swap", swap_path.display()));
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

/// The partitions the kernel lists under a disk's sysfs directory, sorted.
fn partition_names(disk_sys_dir: &Path) -> Result<Vec<String>> {
    let fail = |err: std::io::Error| {
        RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "the kernel lists the disk's partitions",
            err.to_string(),
            "Run recpart on a system with /sys mounted.",
        )
    };
    let mut names = Vec::new();
    for name in target_root::dir_names(disk_sys_dir).map_err(fail)? {
        let child = disk_sys_dir.join(&name);
        // sysfs also contains attribute files and links to other devices.
        // Only child directories can describe this disk's partitions.
        let metadata = fs::symlink_metadata(&child)
            .map_err(|err| fail(target_root::at(&child, err)))?;
        if metadata.is_dir() && target_root::exists(&child.join("partition")).map_err(fail)? {
            names.push(name.to_string_lossy().to_string());
        }
    }
    Ok(names)
}

/// Sorted names of the partitions the kernel currently exposes for `disk`.
pub fn kernel_partition_names(disk: &Path) -> Result<Vec<String>> {
    let Some(name) = disk.file_name() else {
        return Ok(Vec::new());
    };
    partition_names(&Path::new(SYS_BLOCK).join(name))
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
    let read_only = query_disk(path)?.read_only;
    if !read_only {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::InvalidTargetDisk,
        "preflight",
        "target disk is writable",
        format!("{} is read-only", path.display()),
        "Use a writable block device target and retry.",
    ))
}

/// Describe the whole disk at `path` (a partition is refused).
pub(crate) fn query_disk(path: &Path) -> Result<DiskInventory> {
    let canonical = resolve_disk_path(path)?;
    ensure_block_device(&canonical)?;
    let disk = lsblk_disks(Some(&canonical))?
        .into_iter()
        .next()
        .ok_or_else(|| {
            RecpartError::new(
                ErrorCode::InvalidTargetDisk,
                "preflight",
                "lsblk describes the target disk",
                format!("lsblk printed nothing for {}", canonical.display()),
                "Verify the disk path is valid and accessible.",
            )
        })?;
    if !disk.is_whole_disk() {
        return Err(RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "target path resolves to whole-disk device type",
            format!("device type is '{}'", disk.dev_type),
            "Target a disk device (for example /dev/sda), not a partition like /dev/sda1.",
        ));
    }
    if !valid_sector_bytes(disk.log_sec) {
        return Err(RecpartError::new(
            ErrorCode::InvalidTargetDisk,
            "preflight",
            "lsblk reports the disk's logical sector size",
            format!("LOG-SEC is {}", disk.log_sec),
            "Check the disk with 'lsblk -o PATH,LOG-SEC'; recpart needs a power of two of at least 512 bytes.",
        ));
    }
    Ok(disk.into_inventory())
}

/// A logical sector size recpart can plan with.
pub(crate) fn valid_sector_bytes(bytes: u32) -> bool {
    bytes >= 512 && bytes.is_power_of_two()
}

pub fn query_disk_target(path: &Path) -> Result<DiskTarget> {
    Ok(query_disk(path)?.target)
}

/// Whole disks that can be installed to (zram swap devices are left out).
pub fn list_disk_inventory() -> Result<DiskListResult> {
    let mut disks: Vec<DiskInventory> = lsblk_disks(None)?
        .into_iter()
        .filter(LsblkDisk::is_whole_disk)
        .filter(|disk| {
            !disk
                .path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("zram"))
        })
        .map(LsblkDisk::into_inventory)
        .collect();
    disks.sort_by(|a, b| a.target.path.cmp(&b.target.path));
    Ok(DiskListResult {
        schema_version: DISK_LIST_SCHEMA_VERSION,
        disks,
    })
}
