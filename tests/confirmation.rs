use distro_spec::shared::error::ToolErrorCode;
use recpart::build_plan;
use recpart::execute_plan;
use recpart::types::{ApplyOptions, DiskTarget, InstallMode};
use std::path::PathBuf;

#[test]
fn apply_requires_confirmation_token_when_not_dry_run() {
    let disk = DiskTarget {
        path: PathBuf::from("/dev/vda"),
        size_bytes: 250 * 1024 * 1024 * 1024,
        logical_sector_bytes: 512,
        physical_sector_bytes: 4096,
    };
    let plan = build_plan(disk, InstallMode::Mutable).expect("plan");

    let err = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: false,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt"),
        },
    )
    .expect_err("missing confirmation should fail");

    assert_eq!(err.code.code(), "E004");
}
