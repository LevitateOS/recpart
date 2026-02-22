use recpart::build_plan;
use recpart::execute_plan;
use recpart::types::{ApplyOptions, DiskTarget, InstallMode};
use std::path::PathBuf;

fn fake_disk() -> DiskTarget {
    DiskTarget {
        path: PathBuf::from("/dev/vda"),
        size_bytes: 250 * 1024 * 1024 * 1024,
        logical_sector_bytes: 512,
        physical_sector_bytes: 4096,
    }
}

#[test]
fn dry_run_apply_order_is_partition_then_format_then_mount() {
    let plan = build_plan(fake_disk(), InstallMode::Ab).expect("plan");
    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt"),
        },
    )
    .expect("dry-run apply");

    let phases = result
        .steps
        .iter()
        .map(|s| s.phase.as_str())
        .collect::<Vec<_>>();

    let first_format = phases
        .iter()
        .position(|p| *p == "format")
        .expect("format phase exists");
    let first_mount = phases
        .iter()
        .position(|p| *p == "mount")
        .expect("mount phase exists");

    assert!(phases[..first_format].iter().all(|p| *p == "partition"));
    assert!(phases[first_format..first_mount]
        .iter()
        .all(|p| *p == "format"));
    assert!(phases[first_mount..].iter().all(|p| *p == "mount"));
}

#[test]
fn dry_run_partition_sequence_includes_wipefs_and_udevadm() {
    let plan = build_plan(fake_disk(), InstallMode::Ab).expect("plan");
    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt"),
        },
    )
    .expect("dry-run apply");

    let partition_programs = result
        .steps
        .iter()
        .filter(|s| s.phase == "partition")
        .map(|s| s.program.as_str())
        .collect::<Vec<_>>();
    assert_eq!(partition_programs, vec!["wipefs", "sfdisk", "udevadm"]);
}
