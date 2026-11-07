use crate::error::{ErrorCode, RecpartError, Result};
use crate::handoff::build_handoff;
use crate::plan::partition_device;
use crate::policy::NOT_MOUNTED;
use crate::preflight::{
    ensure_disk_not_in_use, ensure_disk_not_mounted, ensure_disk_writable, ensure_mount_root_safe,
    ensure_required_tools, ensure_root_for_apply, kernel_partition_names,
};
use crate::inspect::{resize_support, ResizeTool, MIB};
use crate::types::{
    ApplyOptions, ApplyResult, ChangeKind, CommandRecord, Filesystem, MountMapping, PartitionChange,
    PartitionPlan, PartitionTemplate, Role, APPLY_SCHEMA_VERSION, CONFIRM_TOKEN,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};


/// The stages of apply; a failure is reported with its stage's code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Partition,
    Resize,
    Format,
    Mount,
    Swap,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Partition => "partition",
            Phase::Resize => "resize",
            Phase::Format => "format",
            Phase::Mount => "mount",
            Phase::Swap => "swap",
        }
    }

    fn error_code(self) -> ErrorCode {
        match self {
            Phase::Partition => ErrorCode::PartitionApplyFailed,
            Phase::Resize => ErrorCode::ResizeFailed,
            Phase::Format => ErrorCode::FormatFailed,
            Phase::Mount => ErrorCode::MountFailed,
            Phase::Swap => ErrorCode::SwapFailed,
        }
    }
}

/// Exit statuses that mean success: 0, and for e2fsck also 1 (errors corrected).
const OK: &[i32] = &[0];
const E2FSCK_OK: &[i32] = &[0, 1];

#[derive(Debug, Clone)]
enum ExecutionAction {
    Run {
        phase: Phase,
        program: String,
        args: Vec<String>,
        stdin: Option<String>,
        ok: &'static [i32],
    },
    /// Wait for the kernel to expose exactly the planned partitions of the
    /// target disk. Runs once, after the new table is written and udev settled.
    VerifyPartitions,
    /// Create the mount point and mount the device there.
    Mount { device: String, target: PathBuf },
    /// Activate a swap device or file (recfstab writes its fstab entry from /proc/swaps).
    Swapon { target: String },
}

fn run(phase: Phase, program: &str, args: &[&str]) -> ExecutionAction {
    ExecutionAction::Run {
        phase,
        program: program.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        stdin: None,
        ok: OK,
    }
}

pub fn execute_plan(plan: &PartitionPlan, opts: &ApplyOptions) -> Result<ApplyResult> {
    ensure_mount_root_safe(&opts.mount_root)?;
    let actions = build_apply_actions(plan, &opts.mount_root);
    let programs = required_programs(&actions);

    // The disk checks only read; dry-run reports what apply would refuse
    // instead of stopping, so a preview says whether apply can run.
    let disk_checks: [&dyn Fn() -> Result<()>; 5] = [
        &|| ensure_root_for_apply(),
        &|| ensure_required_tools(&programs),
        &|| ensure_disk_writable(&plan.disk.path),
        &|| ensure_disk_not_mounted(&plan.disk.path),
        &|| ensure_disk_not_in_use(&plan.disk.path),
    ];
    let mut warnings = Vec::new();
    if opts.dry_run && opts.skip_disk_checks {
        // A recorded inventory: there is no live disk to check.
    } else if opts.dry_run {
        warnings.extend(disk_checks.iter().filter_map(|check| check().err()).map(|e| e.to_string()));
    } else {
        ensure_confirmation(opts.confirm_token.as_deref())?;
        for check in disk_checks {
            check()?;
        }
    }

    let mounted: Vec<MountMapping> = actions
        .iter()
        .filter_map(|action| match action {
            ExecutionAction::Mount { device, target } => Some(MountMapping {
                path: target.to_string_lossy().to_string(),
                device: device.clone(),
            }),
            _ => None,
        })
        .collect();
    let swaps: Vec<String> = actions
        .iter()
        .filter_map(|action| match action {
            ExecutionAction::Swapon { target } => Some(target.clone()),
            _ => None,
        })
        .collect();
    let handoff = build_handoff(plan, mounted, swaps);

    let steps = if opts.dry_run {
        actions.iter().map(|action| action.record(true)).collect()
    } else {
        let mut records = Vec::with_capacity(actions.len());
        let mut activated: Vec<Activated> = Vec::new();
        for action in &actions {
            let record = match run_action(action, plan, &opts.mount_root) {
                Ok(record) => record,
                Err(err) => return Err(rollback(err, &activated)),
            };
            match action {
                ExecutionAction::Mount { target, .. } => {
                    activated.push(Activated::Mount(target.to_string_lossy().to_string()))
                }
                ExecutionAction::Swapon { target } => activated.push(Activated::Swap(target.clone())),
                _ => {}
            }
            records.push(record);
        }
        records
    };

    Ok(ApplyResult {
        schema_version: APPLY_SCHEMA_VERSION,
        mode: plan.mode,
        layout_request: plan.layout_request.clone(),
        resolved_layout: plan.resolved_layout.clone(),
        disk: plan.disk.clone(),
        dry_run: opts.dry_run,
        steps,
        partition_map: plan.partitions.clone(),
        formatted_devices: formatted_devices(plan),
        handoff,
        warnings,
        summary: plan.summary.clone(),
        disk_checks_skipped: opts.dry_run && opts.skip_disk_checks,
    })
}

/// What a failed apply undoes, newest first.
enum Activated {
    Mount(String),
    Swap(String),
}

/// Deactivate the swaps and unmount everything this run mounted (reverse
/// order) after a failure and fold the outcome into the returned error.
fn rollback(mut err: RecpartError, activated: &[Activated]) -> RecpartError {
    if activated.is_empty() {
        return err;
    }

    let mut failed = Vec::new();
    let mut undone = Vec::new();
    for item in activated.iter().rev() {
        let (program, target) = match item {
            Activated::Mount(target) => ("umount", target),
            Activated::Swap(target) => ("swapoff", target),
        };
        undone.push(target.clone());
        match Command::new(program).arg(target).output() {
            Ok(output) if output.status.success() => {}
            Ok(output) => failed.push(format!(
                "{target} ({})",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(e) => failed.push(format!("{target} ({e})")),
        }
    }

    if failed.is_empty() {
        err.observed = format!(
            "{}; rolled back mounts: {}",
            err.observed,
            undone.join(", ")
        );
    } else {
        err.observed = format!(
            "{}; rollback could not unmount: {}",
            err.observed,
            failed.join(", ")
        );
    }
    err
}

fn ensure_confirmation(token: Option<&str>) -> Result<()> {
    if matches!(token, Some(CONFIRM_TOKEN)) {
        return Ok(());
    }

    Err(RecpartError::new(
        ErrorCode::MissingConfirmation,
        "apply",
        format!("--confirm {CONFIRM_TOKEN} is provided for destructive apply"),
        format!("received token: {:?}", token),
        format!("Re-run with --confirm {CONFIRM_TOKEN} to acknowledge destructive disk changes."),
    ))
}

/// The filesystem-creation command for a created or formatted partition.
fn format_command(change: &PartitionChange) -> Option<ExecutionAction> {
    let after = change.after.as_ref()?;
    let fs = after.filesystem.as_deref().and_then(Filesystem::from_blkid)?;
    let label = after.fs_label.as_deref().filter(|l| !l.is_empty());
    let (program, mut args): (&str, Vec<&str>) = match fs {
        Filesystem::Vfat => ("mkfs.vfat", vec!["-F", "32"]),
        Filesystem::Ext4 => ("mkfs.ext4", vec!["-F"]),
        Filesystem::Swap => ("mkswap", vec![]),
    };
    if let Some(label) = label {
        args.push(match fs {
            Filesystem::Vfat => "-n",
            _ => "-L",
        });
        args.push(label);
    }
    args.push(&change.device);
    Some(run(Phase::Format, program, &args))
}

/// The commands that resize a partition's filesystem: before the table
/// write for a shrink, after it for a grow.
fn resize_actions(change: &PartitionChange) -> Vec<ExecutionAction> {
    let (Some(before), Some(after)) = (&change.before, &change.after) else {
        return Vec::new();
    };
    let Some(tool) = resize_support(before.filesystem.as_deref()) else {
        return Vec::new();
    };
    let device = change.device.as_str();
    let shrink = after.size_bytes < before.size_bytes;
    let bytes = after.size_bytes.to_string();
    match tool {
        ResizeTool::Ext => {
            let size = format!("{}M", after.size_bytes / MIB);
            let mut fsck = run(Phase::Resize, "e2fsck", &["-f", "-p", device]);
            if let ExecutionAction::Run { ok, .. } = &mut fsck {
                *ok = E2FSCK_OK;
            }
            let resize = if shrink {
                run(Phase::Resize, "resize2fs", &[device, &size])
            } else {
                run(Phase::Resize, "resize2fs", &[device])
            };
            vec![fsck, resize]
        }
        ResizeTool::Fat => vec![run(Phase::Resize, "fatresize", &["--force-yes", "--size", &bytes, device])],
        ResizeTool::Ntfs => {
            // The test run refuses a hibernated or unclean volume before anything changes;
            // the real run reads its confirmation from stdin.
            let size_args: Vec<&str> = if shrink { vec!["--size", &bytes] } else { vec![] };
            let mut test = vec!["--no-action", "--no-progress-bar"];
            test.extend(&size_args);
            test.push(device);
            let mut real = vec!["--no-progress-bar"];
            real.extend(&size_args);
            real.push(device);
            let mut resize = run(Phase::Resize, "ntfsresize", &real);
            if let ExecutionAction::Run { stdin, .. } = &mut resize {
                *stdin = Some("y\n".to_string());
            }
            vec![run(Phase::Resize, "ntfsresize", &test), resize]
        }
    }
}

/// Every step of apply, derived from the plan: shrink filesystems, write the
/// table (after erasing the disk, for a template or a new table), grow
/// filesystems, format each created or formatted partition, mount each
/// partition that has a mount point at `<mount_root><mountpoint>`,
/// parents first, then activate swap.
fn build_apply_actions(plan: &PartitionPlan, mount_root: &Path) -> Vec<ExecutionAction> {
    let disk = plan.disk.path.to_string_lossy().to_string();
    let resized = |grow: bool| {
        plan.summary
            .iter()
            .filter(|c| c.change == ChangeKind::Resized)
            .filter(move |c| match (&c.before, &c.after) {
                (Some(b), Some(a)) => (a.size_bytes > b.size_bytes) == grow,
                _ => false,
            })
            .flat_map(resize_actions)
    };
    let mut actions = Vec::new();
    if plan.wipes_disk {
        // No --force: wipefs then refuses devices that are busy.
        actions.push(run(Phase::Partition, "wipefs", &["-a", &disk]));
    }
    actions.extend(resized(false));
    let mut sfdisk = if plan.wipes_disk {
        run(Phase::Partition, "sfdisk", &[&disk])
    } else {
        // Editing a table: keep every signature on the disk and its partitions.
        run(Phase::Partition, "sfdisk", &["--wipe", "never", "--wipe-partitions", "never", &disk])
    };
    if let ExecutionAction::Run { stdin, .. } = &mut sfdisk {
        *stdin = Some(plan.sfdisk_script.clone());
    }
    actions.push(sfdisk);
    actions.push(run(Phase::Partition, "udevadm", &["settle", "--timeout=10"]));
    actions.push(ExecutionAction::VerifyPartitions);
    actions.extend(resized(true));

    actions.extend(
        plan.summary
            .iter()
            .filter(|c| matches!(c.change, ChangeKind::Created | ChangeKind::Formatted))
            .filter_map(format_command),
    );

    let sysroot = install_root(mount_root);
    let mut mounted: Vec<&PartitionTemplate> = plan
        .partitions
        .iter()
        .filter(|part| part.mountpoint != NOT_MOUNTED)
        .collect();
    mounted.sort_by_key(|part| part.mountpoint.trim_end_matches('/').matches('/').count());
    let in_sysroot = |path: &str| {
        let relative = path.trim_start_matches('/');
        if relative.is_empty() {
            sysroot.clone()
        } else {
            sysroot.join(relative)
        }
    };
    for part in mounted {
        actions.push(ExecutionAction::Mount {
            device: partition_device(&plan.disk.path, part.index.into()),
            target: in_sysroot(&part.mountpoint),
        });
    }

    for part in plan.partitions.iter().filter(|p| p.name == Role::Swap.as_str()) {
        actions.push(ExecutionAction::Swapon {
            target: partition_device(&plan.disk.path, part.index.into()),
        });
    }
    if let Some(file) = &plan.swap_file {
        let path = in_sysroot(&file.path).to_string_lossy().to_string();
        let size = format!("{}MiB", file.size_mb);
        actions.push(run(Phase::Swap, "fallocate", &["-l", &size, &path]));
        actions.push(run(Phase::Swap, "chmod", &["600", &path]));
        actions.push(run(Phase::Swap, "mkswap", &[&path]));
        actions.push(ExecutionAction::Swapon { target: path });
    }

    actions
}

/// The programs `actions` run, plus `umount` for rolling mounts back.
fn required_programs(actions: &[ExecutionAction]) -> Vec<String> {
    let mut programs: Vec<String> = actions
        .iter()
        .filter_map(|action| match action {
            ExecutionAction::Run { program, .. } => Some(program.clone()),
            ExecutionAction::Mount { .. } => Some("mount".to_string()),
            ExecutionAction::Swapon { .. } => Some("swapon".to_string()),
            ExecutionAction::VerifyPartitions => None,
        })
        .chain(["umount".to_string()])
        .chain(
            actions
                .iter()
                .any(|a| matches!(a, ExecutionAction::Swapon { .. }))
                .then(|| "swapoff".to_string()),
        )
        .collect();
    programs.sort();
    programs.dedup();
    programs
}

/// Where the installed system's root is mounted during installation: the mount
/// root itself, so the guides' `recstrap /mnt` is the target.
fn install_root(mount_root: &Path) -> PathBuf {
    mount_root.to_path_buf()
}

fn formatted_devices(plan: &PartitionPlan) -> Vec<String> {
    plan.summary
        .iter()
        .filter(|c| matches!(c.change, ChangeKind::Created | ChangeKind::Formatted))
        .map(|c| c.device.clone())
        .collect()
}

/// Every partition of the table apply writes.
fn final_devices(plan: &PartitionPlan) -> Vec<String> {
    plan.summary
        .iter()
        .filter(|c| c.after.is_some())
        .map(|c| c.device.clone())
        .collect()
}

fn run_action(
    action: &ExecutionAction,
    plan: &PartitionPlan,
    mount_root: &Path,
) -> Result<CommandRecord> {
    let fail = |phase: Phase, expected: String, observed: String| {
        RecpartError::new(
            phase.error_code(),
            "exec",
            expected,
            observed,
            format!(
                "Fix the reported {} failure and retry. Disk: {}. If partial mounts were created under {}, unmount them before retry.",
                phase.as_str(),
                plan.disk.path.display(),
                mount_root.display()
            ),
        )
    };
    let mut record = action.record(false);
    match action {
        ExecutionAction::VerifyPartitions => {
            let found = wait_for_partition_devices(plan, Duration::from_secs(10))?;
            record.status = Some(0);
            record.stdout = Some(found.join(" "));
        }
        ExecutionAction::Mount { device, target } => {
            fs::create_dir_all(target).map_err(|err| {
                fail(Phase::Mount, format!("'{}' can be created", target.display()), err.to_string())
            })?;
            let output = Command::new("mount")
                .arg(device)
                .arg(target)
                .output()
                .map_err(|err| fail(Phase::Mount, "mount starts".to_string(), err.to_string()))?;
            record.status = output.status.code();
            if !output.status.success() {
                return Err(fail(
                    Phase::Mount,
                    "mount exits with status 0".to_string(),
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ));
            }
        }
        ExecutionAction::Swapon { target } => {
            let output = Command::new("swapon")
                .arg(target)
                .output()
                .map_err(|err| fail(Phase::Swap, "swapon starts".to_string(), err.to_string()))?;
            record.status = output.status.code();
            if !output.status.success() {
                return Err(fail(
                    Phase::Swap,
                    "swapon exits with status 0".to_string(),
                    String::from_utf8_lossy(&output.stderr).trim().to_string(),
                ));
            }
        }
        ExecutionAction::Run {
            phase,
            program,
            args,
            stdin,
            ok,
        } => {
            let mut cmd = Command::new(program);
            cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
            if stdin.is_some() {
                cmd.stdin(Stdio::piped());
            }
            let mut child = cmd
                .spawn()
                .map_err(|err| fail(*phase, format!("{program} starts successfully"), err.to_string()))?;
            if let (Some(payload), Some(mut handle)) = (stdin, child.stdin.take()) {
                handle.write_all(payload.as_bytes()).map_err(|err| {
                    fail(*phase, format!("{program} accepts stdin payload"), err.to_string())
                })?;
            }
            let output = child
                .wait_with_output()
                .map_err(|err| fail(*phase, format!("{program} exits cleanly"), err.to_string()))?;
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let status = output.status.code().unwrap_or(-1);
            if !ok.contains(&status) {
                return Err(fail(
                    *phase,
                    format!("{program} exits with status 0"),
                    format!("exit {status}; stderr: {stderr}"),
                ));
            }
            record.status = Some(status);
            record.stdout = (!stdout.is_empty()).then_some(stdout);
            record.stderr = (!stderr.is_empty()).then_some(stderr);
        }
    }
    Ok(record)
}

/// Wait until the kernel exposes exactly the planned partitions as children of
/// the target disk, and each planned device node exists. Returns the kernel's
/// partition names.
fn wait_for_partition_devices(plan: &PartitionPlan, timeout: Duration) -> Result<Vec<String>> {
    let devices = final_devices(plan);
    let expected: Vec<String> = {
        let mut names: Vec<String> = devices
            .iter()
            .filter_map(|dev| Path::new(dev).file_name())
            .map(|name| name.to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    };

    let start = Instant::now();
    loop {
        let kernel = kernel_partition_names(&plan.disk.path);
        let missing_nodes = devices
            .iter()
            .filter(|dev| !Path::new(dev.as_str()).exists())
            .cloned()
            .collect::<Vec<_>>();

        if kernel == expected && missing_nodes.is_empty() {
            return Ok(kernel);
        }

        if start.elapsed() >= timeout {
            return Err(RecpartError::new(
                ErrorCode::PartitionApplyFailed,
                "exec",
                format!(
                    "kernel exposes exactly the planned partitions of {}: {}",
                    plan.disk.path.display(),
                    expected.join(", ")
                ),
                format!(
                    "after {:?}: kernel partitions [{}], missing device nodes [{}]",
                    timeout,
                    kernel.join(", "),
                    missing_nodes.join(", ")
                ),
                "Check dmesg for partition table errors, run 'udevadm settle', then retry.",
            ));
        }

        thread::sleep(Duration::from_millis(250));
    }
}

impl ExecutionAction {
    fn phase(&self) -> Phase {
        match self {
            ExecutionAction::Run { phase, .. } => *phase,
            ExecutionAction::VerifyPartitions => Phase::Partition,
            ExecutionAction::Mount { .. } => Phase::Mount,
            ExecutionAction::Swapon { .. } => Phase::Swap,
        }
    }

    fn program_and_args(&self) -> (String, Vec<String>) {
        match self {
            ExecutionAction::Run { program, args, .. } => (program.clone(), args.clone()),
            ExecutionAction::VerifyPartitions => ("verify-partitions".to_string(), Vec::new()),
            ExecutionAction::Mount { device, target } => (
                "mount".to_string(),
                vec![device.clone(), target.to_string_lossy().to_string()],
            ),
            ExecutionAction::Swapon { target } => ("swapon".to_string(), vec![target.clone()]),
        }
    }

    /// The step as reported in JSON, before it has run.
    fn record(&self, dry_run: bool) -> CommandRecord {
        let (program, args) = self.program_and_args();
        let command = if args.is_empty() {
            program.clone()
        } else {
            format!("{} {}", program, args.join(" "))
        };
        CommandRecord {
            phase: self.phase().as_str().to_string(),
            command,
            program,
            args,
            status: None,
            stdout: None,
            stderr: None,
            dry_run,
        }
    }
}
