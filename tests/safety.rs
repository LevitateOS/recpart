use distro_spec::shared::error::ToolErrorCode;
use recpart::build_plan;
use recpart::execute_plan;
use recpart::plan::partition_device;
use recpart::types::{ApplyOptions, DiskTarget, InstallMode};
use std::path::{Path, PathBuf};

fn fake_disk(path: &str) -> DiskTarget {
    DiskTarget {
        path: PathBuf::from(path),
        size_bytes: 250 * 1024 * 1024 * 1024,
        logical_sector_bytes: 512,
        physical_sector_bytes: 4096,
    }
}

#[test]
fn partition_device_handles_nvme_and_sd_style_names() {
    assert_eq!(partition_device(Path::new("/dev/sda"), 1), "/dev/sda1");
    assert_eq!(
        partition_device(Path::new("/dev/nvme0n1"), 2),
        "/dev/nvme0n1p2"
    );
    assert_eq!(
        partition_device(Path::new("/dev/mmcblk0"), 3),
        "/dev/mmcblk0p3"
    );
}

#[test]
fn apply_rejects_protected_mount_root_even_in_dry_run() {
    let plan = build_plan(fake_disk("/dev/vda"), InstallMode::Mutable).expect("plan");

    let err = execute_plan(
        &plan,
        &ApplyOptions {
            dry_run: true,
            confirm_token: None,
            mount_root: PathBuf::from("/"),
        },
    )
    .expect_err("protected mount root should fail");

    assert_eq!(err.code.code(), "E007");
}

#[test]
fn too_small_disk_fails_plan_generation() {
    let tiny = DiskTarget {
        path: PathBuf::from("/dev/vda"),
        size_bytes: 200 * 1024 * 1024,
        logical_sector_bytes: 512,
        physical_sector_bytes: 4096,
    };

    let err = build_plan(tiny, InstallMode::Ab).expect_err("tiny disk must fail");
    assert_eq!(err.code.code(), "E003");
}
