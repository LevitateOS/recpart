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
fn ab_mode_handoff_contains_slot_context() {
    let plan = build_plan(fake_disk(), InstallMode::Ab).expect("plan");
    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt"),
        },
    )
    .expect("dry run should succeed");

    assert_eq!(
        result.handoff.mode_context.install_target_slot.as_deref(),
        Some("A")
    );
    assert_eq!(
        result.handoff.mode_context.inactive_slot_hint.as_deref(),
        Some("B")
    );
    assert!(result
        .handoff
        .next_commands
        .iter()
        .any(|cmd| cmd.contains("recstrap /mnt/sysroot")));
}

#[test]
fn mutable_mode_handoff_has_no_slot_context() {
    let plan = build_plan(fake_disk(), InstallMode::Mutable).expect("plan");
    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt"),
        },
    )
    .expect("dry run should succeed");

    assert!(result.handoff.mode_context.install_target_slot.is_none());
}
