use recpart::build_plan;
use recpart::types::{DiskTarget, InstallMode};
use std::path::PathBuf;

fn fake_disk() -> DiskTarget {
    DiskTarget {
        path: PathBuf::from("/dev/vda"),
        size_bytes: 200 * 1024 * 1024 * 1024,
        logical_sector_bytes: 512,
        physical_sector_bytes: 4096,
    }
}

#[test]
fn ab_mode_plan_has_expected_partitions() {
    let plan = build_plan(fake_disk(), InstallMode::Ab).expect("ab plan should build");
    assert_eq!(plan.partitions.len(), 4);
    assert_eq!(plan.partitions[0].label, "EFI");
    assert_eq!(plan.partitions[1].label, "ROOT_A");
    assert_eq!(plan.partitions[2].label, "ROOT_B");
    assert_eq!(plan.partitions[3].label, "STATE");
}

#[test]
fn mutable_mode_plan_has_expected_partitions() {
    let plan = build_plan(fake_disk(), InstallMode::Mutable).expect("mutable plan should build");
    assert_eq!(plan.partitions.len(), 2);
    assert_eq!(plan.partitions[0].label, "EFI");
    assert_eq!(plan.partitions[1].label, "ROOT");
}
