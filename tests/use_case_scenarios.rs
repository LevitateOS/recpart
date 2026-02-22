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
fn uc_001_ab_plan_contains_expected_partition_labels() {
    let plan = build_plan(fake_disk(), InstallMode::Ab).expect("ab plan should build");
    let labels = plan
        .partitions
        .iter()
        .map(|p| p.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(labels, vec!["EFI", "ROOT_A", "ROOT_B", "STATE"]);
}

#[test]
fn uc_002_ab_dry_run_has_slot_context_and_state_mount() {
    let plan = build_plan(fake_disk(), InstallMode::Ab).expect("ab plan should build");
    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt/recpart-uc"),
        },
    )
    .expect("ab dry-run should succeed");

    assert_eq!(result.mode, InstallMode::Ab);
    assert_eq!(
        result.handoff.mode_context.install_target_slot.as_deref(),
        Some("A")
    );
    assert_eq!(
        result.handoff.mode_context.inactive_slot_hint.as_deref(),
        Some("B")
    );
    assert!(
        result.mounted.iter().any(|m| m.path.ends_with("/state")),
        "state mount should be present in AB mode"
    );
}

#[test]
fn uc_005_mutable_dry_run_has_single_root_handoff() {
    let plan = build_plan(fake_disk(), InstallMode::Mutable).expect("mutable plan should build");
    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt/recpart-uc"),
        },
    )
    .expect("mutable dry-run should succeed");

    assert_eq!(result.mode, InstallMode::Mutable);
    assert!(result.handoff.mode_context.install_target_slot.is_none());
    assert!(
        !result.mounted.iter().any(|m| m.path.ends_with("/state")),
        "mutable mode should not mount /state"
    );
    assert!(
        result
            .handoff
            .next_commands
            .iter()
            .any(|cmd| cmd.contains("recstrap /mnt/recpart-uc/sysroot")),
        "handoff should include recstrap target"
    );
}
