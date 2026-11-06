use crate::error::{ErrorCode, RecpartError, Result};
use crate::handoff::build_handoff;
use crate::plan::partition_device;
use crate::policy::NOT_MOUNTED;
use crate::preflight::{
    ensure_disk_not_in_use, ensure_disk_not_mounted, ensure_disk_writable, ensure_mount_root_safe,
    ensure_required_tools, ensure_root_for_apply, kernel_partition_names,
};
use crate::types::{
    ApplyOptions, ApplyResult, CommandRecord, MountMapping, PartitionPlan, PartitionTemplate,
    APPLY_SCHEMA_VERSION,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CONFIRM_TOKEN: &str = "DESTROY";

/// The three stages of apply; a failure is reported with its stage's code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Partition,
    Format,
    Mount,
}

impl Phase {
    fn as_str(self) -> &'static str {
        match self {
            Phase::Partition => "partition",
            Phase::Format => "format",
            Phase::Mount => "mount",
        }
    }

    fn error_code(self) -> ErrorCode {
        match self {
            Phase::Partition => ErrorCode::PartitionApplyFailed,
            Phase::Format => ErrorCode::FormatFailed,
            Phase::Mount => ErrorCode::MountFailed,
        }
    }
}

#[derive(Debug, Clone)]
enum ExecutionAction {
    Run {
        phase: Phase,
        program: String,
        args: Vec<String>,
        stdin: Option<String>,
    },
    /// Wait for the kernel to expose exactly the planned partitions of the
    /// target disk. Runs once, after the new table is written and udev settled.
    VerifyPartitions,
    /// Create the mount point and mount the device there.
    Mount { device: String, target: PathBuf },
}

pub fn execute_plan(plan: &PartitionPlan, opts: &ApplyOptions) -> Result<ApplyResult> {
    ensure_mount_root_safe(&opts.mount_root)?;

    // The disk checks only read; dry-run reports what apply would refuse
    // instead of stopping, so a preview says whether apply can run.
    let disk_checks: [&dyn Fn() -> Result<()>; 5] = [
        &|| ensure_root_for_apply(),
        &|| ensure_required_tools(),
        &|| ensure_disk_writable(&plan.disk.path),
        &|| ensure_disk_not_mounted(&plan.disk.path),
        &|| ensure_disk_not_in_use(&plan.disk.path),
    ];
    let mut warnings = Vec::new();
    if opts.dry_run {
        warnings.extend(disk_checks.iter().filter_map(|check| check().err()).map(|e| e.to_string()));
    } else {
        ensure_confirmation(opts.confirm_token.as_deref())?;
        for check in disk_checks {
            check()?;
        }
    }

    let actions = build_apply_actions(plan, &opts.mount_root)?;
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
    let handoff = build_handoff(plan, mounted.clone());

    let steps = if opts.dry_run {
        actions.iter().map(|action| action.record(true)).collect()
    } else {
        let mut records = Vec::with_capacity(actions.len());
        let mut mounted_targets: Vec<String> = Vec::new();
        for action in &actions {
            let record = match run_action(action, plan, &opts.mount_root) {
                Ok(record) => record,
                Err(err) => return Err(rollback_mounts(err, &mounted_targets)),
            };
            if let ExecutionAction::Mount { target, .. } = action {
                mounted_targets.push(target.to_string_lossy().to_string());
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
        mounted,
        handoff,
        warnings,
    })
}

/// Unmount everything this run mounted (reverse order) after a failure and
/// fold the outcome into the returned error.
fn rollback_mounts(mut err: RecpartError, mounted_targets: &[String]) -> RecpartError {
    if mounted_targets.is_empty() {
        return err;
    }

    let mut failed = Vec::new();
    for target in mounted_targets.iter().rev() {
        match Command::new("umount").arg(target).output() {
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
            mounted_targets.join(", ")
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

/// The filesystem-creation command for a planned partition.
fn format_command(part: &PartitionTemplate, device: String) -> Result<(String, Vec<String>)> {
    let (program, args) = match part.filesystem.as_str() {
        "vfat" => ("mkfs.vfat", vec!["-F", "32", "-n", part.label.as_str()]),
        "ext4" => ("mkfs.ext4", vec!["-F", "-L", part.label.as_str()]),
        other => {
            return Err(RecpartError::new(
                ErrorCode::Internal,
                "exec",
                "planned filesystems are vfat or ext4",
                format!("partition {} uses '{other}'", part.name),
                "Fix the partition policy.",
            ))
        }
    };
    let mut args: Vec<String> = args.into_iter().map(str::to_string).collect();
    args.push(device);
    Ok((program.to_string(), args))
}

/// Every step of apply, derived from the plan's partition templates:
/// partition the disk, format each partition as planned, then mount each
/// partition that has a mount point at `<mount_root>/sysroot<mountpoint>`,
/// parents first.
fn build_apply_actions(plan: &PartitionPlan, mount_root: &Path) -> Result<Vec<ExecutionAction>> {
    let disk = plan.disk.path.to_string_lossy().to_string();
    let mut actions = vec![
        ExecutionAction::Run {
            phase: Phase::Partition,
            program: "wipefs".to_string(),
            // No --force: wipefs then refuses devices that are busy.
            args: vec!["-a".to_string(), disk.clone()],
            stdin: None,
        },
        ExecutionAction::Run {
            phase: Phase::Partition,
            program: "sfdisk".to_string(),
            args: vec![disk],
            stdin: Some(plan.sfdisk_script.clone()),
        },
        ExecutionAction::Run {
            phase: Phase::Partition,
            program: "udevadm".to_string(),
            args: vec!["settle".to_string(), "--timeout=10".to_string()],
            stdin: None,
        },
        ExecutionAction::VerifyPartitions,
    ];

    for part in &plan.partitions {
        let (program, args) = format_command(part, partition_device(&plan.disk.path, part.index))?;
        actions.push(ExecutionAction::Run {
            phase: Phase::Format,
            program,
            args,
            stdin: None,
        });
    }

    let sysroot = install_root(mount_root);
    let mut mounted: Vec<&PartitionTemplate> = plan
        .partitions
        .iter()
        .filter(|part| part.mountpoint != NOT_MOUNTED)
        .collect();
    mounted.sort_by_key(|part| part.mountpoint.trim_end_matches('/').matches('/').count());
    for part in mounted {
        let relative = part.mountpoint.trim_start_matches('/');
        actions.push(ExecutionAction::Mount {
            device: partition_device(&plan.disk.path, part.index),
            target: if relative.is_empty() {
                sysroot.clone()
            } else {
                sysroot.join(relative)
            },
        });
    }

    Ok(actions)
}

/// Where the installed system's root is mounted during installation.
pub fn install_root(mount_root: &Path) -> PathBuf {
    mount_root.join("sysroot")
}

fn formatted_devices(plan: &PartitionPlan) -> Vec<String> {
    plan.partitions
        .iter()
        .map(|part| partition_device(&plan.disk.path, part.index))
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
        ExecutionAction::Run {
            phase,
            program,
            args,
            stdin,
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
            if !output.status.success() {
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
    let devices = formatted_devices(plan);
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
