use recpart::build_plan;
use recpart::types::{DiskTarget, InstallMode};
use std::path::PathBuf;

#[test]
fn same_input_produces_identical_plan() {
    let disk = DiskTarget {
        path: PathBuf::from("/dev/vda"),
        size_bytes: 250 * 1024 * 1024 * 1024,
        logical_sector_bytes: 512,
        physical_sector_bytes: 4096,
    };

    let plan_a = build_plan(disk.clone(), InstallMode::Ab).expect("first plan");
    let plan_b = build_plan(disk, InstallMode::Ab).expect("second plan");

    assert_eq!(plan_a.sfdisk_script, plan_b.sfdisk_script);

    let json_a = serde_json::to_string(&plan_a).expect("serialize a");
    let json_b = serde_json::to_string(&plan_b).expect("serialize b");
    assert_eq!(json_a, json_b);
}
