use distro_spec::shared::error::ToolErrorCode;
use recpart::build_plan_with_layout_request;
use recpart::types::{DiskTarget, InstallMode, LayoutRequest};
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
fn mutable_rejects_ab_only_layout_fields() {
    let err = build_plan_with_layout_request(
        fake_disk(),
        InstallMode::Mutable,
        LayoutRequest {
            root_a_size_mb: Some(10_240),
            ..LayoutRequest::default()
        },
    )
    .expect_err("mutable mode should reject ab-only fields");

    assert_eq!(err.code.code(), "E003");
}

#[test]
fn ab_rejects_mutable_only_layout_fields() {
    let err = build_plan_with_layout_request(
        fake_disk(),
        InstallMode::Ab,
        LayoutRequest {
            root_size_mb: Some(20_480),
            ..LayoutRequest::default()
        },
    )
    .expect_err("ab mode should reject mutable-only fields");

    assert_eq!(err.code.code(), "E003");
}

#[test]
fn ab_custom_layout_request_is_reflected_in_plan() {
    let plan = build_plan_with_layout_request(
        fake_disk(),
        InstallMode::Ab,
        LayoutRequest {
            efi_size_mb: Some(512),
            root_a_size_mb: Some(12_288),
            root_b_size_mb: Some(16_384),
            state_size_mb: Some(8_192),
            ..LayoutRequest::default()
        },
    )
    .expect("plan should build");

    assert_eq!(plan.layout_request.efi_size_mb, Some(512));
    assert_eq!(plan.resolved_layout.efi_size_mb, 512);
    assert_eq!(plan.resolved_layout.root_a_size_mb, Some(12_288));
    assert_eq!(plan.resolved_layout.root_b_size_mb, Some(16_384));
    assert_eq!(plan.resolved_layout.state_size_mb, Some(8_192));

    let sizes = plan
        .partitions
        .iter()
        .map(|p| p.size_mb)
        .collect::<Vec<_>>();
    assert_eq!(
        sizes,
        vec![Some(512), Some(12_288), Some(16_384), Some(8_192)]
    );
}

#[test]
fn mutable_custom_root_size_is_reflected_in_plan() {
    let plan = build_plan_with_layout_request(
        fake_disk(),
        InstallMode::Mutable,
        LayoutRequest {
            efi_size_mb: Some(768),
            root_size_mb: Some(32_768),
            ..LayoutRequest::default()
        },
    )
    .expect("plan should build");

    assert_eq!(plan.resolved_layout.efi_size_mb, 768);
    assert_eq!(plan.resolved_layout.root_size_mb, Some(32_768));
    assert_eq!(plan.partitions[0].size_mb, Some(768));
    assert_eq!(plan.partitions[1].size_mb, Some(32_768));
}

#[test]
fn identical_layout_request_is_deterministic() {
    let request = LayoutRequest {
        efi_size_mb: Some(512),
        root_a_size_mb: Some(12_288),
        root_b_size_mb: Some(16_384),
        state_size_mb: None,
        root_size_mb: None,
    };

    let a = build_plan_with_layout_request(fake_disk(), InstallMode::Ab, request.clone())
        .expect("first plan");
    let b =
        build_plan_with_layout_request(fake_disk(), InstallMode::Ab, request).expect("second plan");

    assert_eq!(a.sfdisk_script, b.sfdisk_script);
    assert_eq!(a.resolved_layout, b.resolved_layout);
    assert_eq!(a.layout_request, b.layout_request);
}
