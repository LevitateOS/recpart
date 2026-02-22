use crate::error::{ErrorCode, RecpartError, Result};
use crate::handoff::build_handoff;
use crate::plan::partition_device;
use crate::preflight::{
    ensure_block_device, ensure_disk_not_mounted, ensure_disk_writable, ensure_mount_root_safe,
    ensure_required_tools, ensure_root_for_apply,
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
    for action in &actions {
        let record = run_action(action, plan, &opts.mount_root)?;
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
    let mut actions = Vec::new();

    actions.push(ExecutionAction::Run {
        phase: "partition".to_string(),
        program: "wipefs".to_string(),
        args: vec![
            "-a".to_string(),
            "--force".to_string(),
            plan.disk.path.to_string_lossy().to_string(),
        ],
        stdin: None,
    });

    actions.push(ExecutionAction::Run {
        phase: "partition".to_string(),
        program: "sfdisk".to_string(),
        args: vec![plan.disk.path.to_string_lossy().to_string()],
        stdin: Some(plan.sfdisk_script.clone()),
    });

    actions.push(ExecutionAction::Run {
        phase: "partition".to_string(),
        program: "udevadm".to_string(),
        args: vec!["settle".to_string(), "--timeout=10".to_string()],
        stdin: None,
    });

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
                command: format!("mkdir -p {}", path.display()),
                program: "mkdir".to_string(),
                args: vec!["-p".to_string(), path.to_string_lossy().to_string()],
                status: Some(0),
                stdout: None,
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

            if phase == "partition" {
                wait_for_partition_devices(&formatted_devices(plan), Duration::from_secs(8))?;
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

fn wait_for_partition_devices(devices: &[String], timeout: Duration) -> Result<()> {
    let start = Instant::now();
    loop {
        let missing = devices
            .iter()
            .filter(|dev| !Path::new(dev.as_str()).exists())
            .cloned()
            .collect::<Vec<_>>();

        if missing.is_empty() {
            return Ok(());
        }

        if start.elapsed() >= timeout {
            return Err(RecpartError::new(
                ErrorCode::PartitionApplyFailed,
                "exec",
                "partition devices appear after partition table write",
                format!(
                    "missing devices after {:?}: {}",
                    timeout,
                    missing.join(", ")
                ),
                "Wait for udev to settle, verify kernel sees the new partition table, then retry.",
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
        }
    }

    fn program_name(&self) -> &str {
        match self {
            ExecutionAction::Run { program, .. } => program,
            ExecutionAction::CreateDir { .. } => "mkdir",
        }
    }

    fn args(&self) -> Vec<String> {
        match self {
            ExecutionAction::Run { args, .. } => args.clone(),
            ExecutionAction::CreateDir { path, .. } => {
                vec!["-p".to_string(), path.to_string_lossy().to_string()]
            }
        }
    }
}
