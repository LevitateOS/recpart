use recpart::build_plan;
use recpart::execute_plan;
use recpart::types::{ApplyOptions, DiskTarget, InstallMode};
use std::path::PathBuf;

#[test]
fn plan_and_apply_include_schema_versions() {
    let disk = DiskTarget {
        path: PathBuf::from("/dev/vda"),
        size_bytes: 250 * 1024 * 1024 * 1024,
        logical_sector_bytes: 512,
        physical_sector_bytes: 4096,
    };

    let plan = build_plan(disk, InstallMode::Ab).expect("plan");
    let plan_json = serde_json::to_value(&plan).expect("plan json");
    assert_eq!(
        plan_json.get("schema_version").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert!(plan_json.get("layout_request").is_some());
    assert!(plan_json.get("resolved_layout").is_some());

    let apply = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt"),
        },
    )
    .expect("apply dry-run");

    let apply_json = serde_json::to_value(&apply).expect("apply json");
    assert_eq!(
        apply_json.get("schema_version").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert!(apply_json.get("layout_request").is_some());
    assert!(apply_json.get("resolved_layout").is_some());
    assert!(apply_json.get("partition_map").is_some());
    assert!(apply_json.get("handoff").is_some());
}
