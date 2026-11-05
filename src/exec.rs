use crate::error::{ErrorCode, RecpartError, Result};
use crate::handoff::build_handoff;
use crate::plan::partition_device;
use crate::preflight::{
    ensure_block_device, ensure_disk_not_in_use, ensure_disk_not_mounted, ensure_disk_writable,
    ensure_mount_root_safe, ensure_required_tools, ensure_root_for_apply, kernel_partition_names,
};
use crate::types::{
    ApplyOptions, ApplyResult, CommandRecord, InstallMode, MountMapping, PartitionPlan,
    APPLY_SCHEMA_VERSION,
};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const CONFIRM_TOKEN: &str = "DESTROY";

#[derive(Debug, Clone)]
enum ExecutionAction {
    Run {
        phase: String,
        program: String,
        args: Vec<String>,
        stdin: Option<String>,
    },
    CreateDir {
        phase: String,
        path: PathBuf,
    },
    /// Wait for the kernel to expose exactly the planned partitions of the
    /// target disk. Runs once, after the new table is written and udev settled.
    VerifyPartitions {
        phase: String,
    },
}

pub fn execute_plan(plan: &PartitionPlan, opts: &ApplyOptions) -> Result<ApplyResult> {
    execute_plan_with_reporter(plan, opts, None)
}

pub fn execute_plan_with_reporter(
    plan: &PartitionPlan,
    opts: &ApplyOptions,
    mut reporter: Option<&mut dyn FnMut(&CommandRecord)>,
) -> Result<ApplyResult> {
    ensure_mount_root_safe(&opts.mount_root)?;

    if !opts.dry_run {
        ensure_confirmation(opts.confirm_token.as_deref())?;
        ensure_root_for_apply()?;
        ensure_required_tools()?;
        ensure_block_device(&plan.disk.path)?;
        ensure_disk_writable(&plan.disk.path)?;
        ensure_disk_not_mounted(&plan.disk.path)?;
        ensure_disk_not_in_use(&plan.disk.path)?;
    }

    let actions = build_apply_actions(plan, &opts.mount_root);
    let mounted = mount_map_for_plan(plan, &opts.mount_root);
    let handoff = build_handoff(plan, mounted.clone());
    let formatted_devices = formatted_devices(plan);

    if opts.dry_run {
        let steps = actions
            .iter()
            .map(|action| CommandRecord {
                phase: action.phase().to_string(),
                command: action.rendered(),
                program: action.program_name().to_string(),
                args: action.args(),
                status: None,
                stdout: None,
                stderr: None,
                dry_run: true,
            })
            .collect::<Vec<_>>();

        return Ok(ApplyResult {
            schema_version: APPLY_SCHEMA_VERSION,
            mode: plan.mode,
            layout_request: plan.layout_request.clone(),
            resolved_layout: plan.resolved_layout.clone(),
            disk: plan.disk.clone(),
            dry_run: true,
            steps,
            partition_map: plan.partitions.clone(),
            formatted_devices,
            mounted,
            handoff,
            warnings: vec![],
        });
    }

    let mut records = Vec::with_capacity(actions.len());
    let mut mounted_targets: Vec<String> = Vec::new();
    for action in &actions {
        let record = match run_action(action, plan, &opts.mount_root) {
            Ok(record) => record,
            Err(err) => return Err(rollback_mounts(err, &mounted_targets)),
        };
        if let ExecutionAction::Run { program, args, .. } = action {
            if program == "mount" {
                if let Some(target) = args.last() {
                    mounted_targets.push(target.clone());
                }
            }
        }
        if let Some(cb) = reporter.as_deref_mut() {
            cb(&record);
        }
        records.push(record);
    }

    Ok(ApplyResult {
        schema_version: APPLY_SCHEMA_VERSION,
        mode: plan.mode,
        layout_request: plan.layout_request.clone(),
        resolved_layout: plan.resolved_layout.clone(),
        disk: plan.disk.clone(),
        dry_run: false,
        steps: records,
        partition_map: plan.partitions.clone(),
        formatted_devices,
        mounted,
        handoff,
        warnings: vec![],
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
        let ok = Command::new("umount")
            .arg(target)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !ok {
            failed.push(target.clone());
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

fn build_apply_actions(plan: &PartitionPlan, mount_root: &Path) -> Vec<ExecutionAction> {
    let disk = plan.disk.path.to_string_lossy().to_string();
    let mut actions = vec![
        ExecutionAction::Run {
            phase: "partition".to_string(),
            program: "wipefs".to_string(),
            // No --force: wipefs then refuses devices that are busy.
            args: vec!["-a".to_string(), disk.clone()],
            stdin: None,
        },
        ExecutionAction::Run {
            phase: "partition".to_string(),
            program: "sfdisk".to_string(),
            args: vec![disk],
            stdin: Some(plan.sfdisk_script.clone()),
        },
        ExecutionAction::Run {
            phase: "partition".to_string(),
            program: "udevadm".to_string(),
            args: vec!["settle".to_string(), "--timeout=10".to_string()],
            stdin: None,
        },
        ExecutionAction::VerifyPartitions {
            phase: "partition".to_string(),
        },
    ];

    let efi = partition_device(&plan.disk.path, 1);
    actions.push(ExecutionAction::Run {
        phase: "format".to_string(),
        program: "mkfs.vfat".to_string(),
        args: vec![
            "-F".to_string(),
            "32".to_string(),
            "-n".to_string(),
            "EFI".to_string(),
            efi.clone(),
        ],
        stdin: None,
    });

    match plan.mode {
        InstallMode::Ab => {
            let root_a = partition_device(&plan.disk.path, 2);
            let root_b = partition_device(&plan.disk.path, 3);
            let state = partition_device(&plan.disk.path, 4);

            for (label, dev) in [
                ("ROOT_A", root_a.clone()),
                ("ROOT_B", root_b),
                ("STATE", state.clone()),
            ] {
                actions.push(ExecutionAction::Run {
                    phase: "format".to_string(),
                    program: "mkfs.ext4".to_string(),
                    args: vec!["-F".to_string(), "-L".to_string(), label.to_string(), dev],
                    stdin: None,
                });
            }

            let sysroot = mount_root.join("sysroot");
            actions.push(ExecutionAction::CreateDir {
                phase: "mount".to_string(),
                path: sysroot.clone(),
            });
            actions.push(ExecutionAction::Run {
                phase: "mount".to_string(),
                program: "mount".to_string(),
                args: vec![root_a, sysroot.to_string_lossy().to_string()],
                stdin: None,
            });

            let boot = sysroot.join("boot");
            actions.push(ExecutionAction::CreateDir {
                phase: "mount".to_string(),
                path: boot.clone(),
            });
            actions.push(ExecutionAction::Run {
                phase: "mount".to_string(),
                program: "mount".to_string(),
                args: vec![efi, boot.to_string_lossy().to_string()],
                stdin: None,
            });

            let state_mount = sysroot.join("state");
            actions.push(ExecutionAction::CreateDir {
                phase: "mount".to_string(),
                path: state_mount.clone(),
            });
            actions.push(ExecutionAction::Run {
                phase: "mount".to_string(),
                program: "mount".to_string(),
                args: vec![state, state_mount.to_string_lossy().to_string()],
                stdin: None,
            });
        }
        InstallMode::Mutable => {
            let root = partition_device(&plan.disk.path, 2);
            actions.push(ExecutionAction::Run {
                phase: "format".to_string(),
                program: "mkfs.ext4".to_string(),
                args: vec![
                    "-F".to_string(),
                    "-L".to_string(),
                    "ROOT".to_string(),
                    root.clone(),
                ],
                stdin: None,
            });

            let sysroot = mount_root.join("sysroot");
            actions.push(ExecutionAction::CreateDir {
                phase: "mount".to_string(),
                path: sysroot.clone(),
            });
            actions.push(ExecutionAction::Run {
                phase: "mount".to_string(),
                program: "mount".to_string(),
                args: vec![root, sysroot.to_string_lossy().to_string()],
                stdin: None,
            });

            let boot = sysroot.join("boot");
            actions.push(ExecutionAction::CreateDir {
                phase: "mount".to_string(),
                path: boot.clone(),
            });
            actions.push(ExecutionAction::Run {
                phase: "mount".to_string(),
                program: "mount".to_string(),
                args: vec![efi, boot.to_string_lossy().to_string()],
                stdin: None,
            });
        }
    }

    actions
}

fn formatted_devices(plan: &PartitionPlan) -> Vec<String> {
    plan.partitions
        .iter()
        .map(|part| partition_device(&plan.disk.path, part.index))
        .collect()
}

fn mount_map_for_plan(plan: &PartitionPlan, mount_root: &Path) -> Vec<MountMapping> {
    match plan.mode {
        InstallMode::Mutable => vec![
            MountMapping {
                path: mount_root.join("sysroot").to_string_lossy().to_string(),
                device: partition_device(&plan.disk.path, 2),
            },
            MountMapping {
                path: mount_root
                    .join("sysroot")
                    .join("boot")
                    .to_string_lossy()
                    .to_string(),
                device: partition_device(&plan.disk.path, 1),
            },
        ],
        InstallMode::Ab => vec![
            MountMapping {
                path: mount_root.join("sysroot").to_string_lossy().to_string(),
                device: partition_device(&plan.disk.path, 2),
            },
            MountMapping {
                path: mount_root
                    .join("sysroot")
                    .join("boot")
                    .to_string_lossy()
                    .to_string(),
                device: partition_device(&plan.disk.path, 1),
            },
            MountMapping {
                path: mount_root
                    .join("sysroot")
                    .join("state")
                    .to_string_lossy()
                    .to_string(),
                device: partition_device(&plan.disk.path, 4),
            },
        ],
    }
}

fn run_action(
    action: &ExecutionAction,
    plan: &PartitionPlan,
    mount_root: &Path,
) -> Result<CommandRecord> {
    match action {
        ExecutionAction::CreateDir { phase, path } => {
            fs::create_dir_all(path).map_err(|err| {
                RecpartError::new(
                    ErrorCode::MountFailed,
                    "exec",
                    format!("directory '{}' can be created", path.display()),
                    err.to_string(),
                    "Ensure mount root is writable and path permissions are correct.",
                )
            })?;

            Ok(CommandRecord {
                phase: phase.clone(),
                command: action.rendered(),
                program: "mkdir".to_string(),
                args: vec!["-p".to_string(), path.to_string_lossy().to_string()],
                status: Some(0),
                stdout: None,
                stderr: None,
                dry_run: false,
            })
        }
        ExecutionAction::VerifyPartitions { phase } => {
            let found = wait_for_partition_devices(plan, Duration::from_secs(10))?;
            Ok(CommandRecord {
                phase: phase.clone(),
                command: action.rendered(),
                program: action.program_name().to_string(),
                args: action.args(),
                status: Some(0),
                stdout: Some(found.join(" ")),
                stderr: None,
                dry_run: false,
            })
        }
        ExecutionAction::Run {
            phase,
            program,
            args,
            stdin,
        } => {
            let mut cmd = Command::new(program);
            cmd.args(args);
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
            if stdin.is_some() {
                cmd.stdin(Stdio::piped());
            }

            let mut child = cmd.spawn().map_err(|err| {
                RecpartError::new(
                    classify_phase_error(phase),
                    "exec",
                    format!("{} starts successfully", program),
                    err.to_string(),
                    "Verify command exists and permissions are sufficient.",
                )
            })?;

            if let Some(stdin_payload) = stdin {
                if let Some(mut handle) = child.stdin.take() {
                    handle.write_all(stdin_payload.as_bytes()).map_err(|err| {
                        RecpartError::new(
                            classify_phase_error(phase),
                            "exec",
                            format!("{} accepts stdin payload", program),
                            err.to_string(),
                            "Inspect generated partition script and command compatibility.",
                        )
                    })?;
                }
            }

            let output = child.wait_with_output().map_err(|err| {
                RecpartError::new(
                    classify_phase_error(phase),
                    "exec",
                    format!("{} exits cleanly", program),
                    err.to_string(),
                    "Inspect system logs and command invocation.",
                )
            })?;

            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let status = output.status.code().unwrap_or(-1);

            if !output.status.success() {
                return Err(RecpartError::new(
                    classify_phase_error(phase),
                    "exec",
                    format!("{} exits with status 0", program),
                    format!("exit {status}; stderr: {stderr}"),
                    format!(
                        "Fix the reported {} failure and retry. Disk: {}. If partial mounts were created under {}, unmount them before retry.",
                        phase,
                        plan.disk.path.display(),
                        mount_root.display()
                    ),
                ));
            }

            Ok(CommandRecord {
                phase: phase.clone(),
                command: action.rendered(),
                program: program.clone(),
                args: args.clone(),
                status: Some(status),
                stdout: if stdout.is_empty() {
                    None
                } else {
                    Some(stdout)
                },
                stderr: if stderr.is_empty() {
                    None
                } else {
                    Some(stderr)
                },
                dry_run: false,
            })
        }
    }
}

fn classify_phase_error(phase: &str) -> ErrorCode {
    match phase {
        "partition" => ErrorCode::PartitionApplyFailed,
        "format" => ErrorCode::FormatFailed,
        "mount" => ErrorCode::MountFailed,
        _ => ErrorCode::Internal,
    }
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
                "Check dmesg for partition table errors, run 'partprobe' or 'udevadm settle', then retry.",
            ));
        }

        thread::sleep(Duration::from_millis(250));
    }
}

impl ExecutionAction {
    fn phase(&self) -> &str {
        match self {
            ExecutionAction::Run { phase, .. } => phase,
            ExecutionAction::CreateDir { phase, .. } => phase,
            ExecutionAction::VerifyPartitions { phase } => phase,
        }
    }

    fn rendered(&self) -> String {
        match self {
            ExecutionAction::Run { program, args, .. } => {
                if args.is_empty() {
                    program.clone()
                } else {
                    format!("{} {}", program, args.join(" "))
                }
            }
            ExecutionAction::CreateDir { path, .. } => format!("mkdir -p {}", path.display()),
            ExecutionAction::VerifyPartitions { .. } => {
                "verify-partitions (kernel exposes planned partitions)".to_string()
            }
        }
    }

    fn program_name(&self) -> &str {
        match self {
            ExecutionAction::Run { program, .. } => program,
            ExecutionAction::CreateDir { .. } => "mkdir",
            ExecutionAction::VerifyPartitions { .. } => "verify-partitions",
        }
    }

    fn args(&self) -> Vec<String> {
        match self {
            ExecutionAction::Run { args, .. } => args.clone(),
            ExecutionAction::CreateDir { path, .. } => {
                vec!["-p".to_string(), path.to_string_lossy().to_string()]
            }
            ExecutionAction::VerifyPartitions { .. } => Vec::new(),
        }
    }
}
