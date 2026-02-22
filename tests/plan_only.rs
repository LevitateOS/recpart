use recpart::build_plan;
use recpart::execute_plan;
use recpart::types::{ApplyOptions, DiskTarget, InstallMode};
use std::path::PathBuf;

#[test]
fn dry_run_does_not_require_confirmation_and_marks_all_steps_dry() {
    let disk = DiskTarget {
        path: PathBuf::from("/dev/vda"),
        size_bytes: 250 * 1024 * 1024 * 1024,
        logical_sector_bytes: 512,
        physical_sector_bytes: 4096,
    };

    let plan = build_plan(disk, InstallMode::Mutable).expect("plan");
    let result = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/mnt"),
        },
    )
    .expect("dry-run apply");

    assert!(result.dry_run);
    assert!(result.steps.iter().all(|s| s.dry_run));
    assert!(result.steps.iter().all(|s| s.status.is_none()));
}
