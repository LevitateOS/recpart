use distro_spec::shared::error::ToolErrorCode;
use recpart::{ErrorCode, RecpartError};

#[test]
fn error_payload_contains_actionable_fields() {
    let err = RecpartError::new(
        ErrorCode::PlanGeneration,
        "plan",
        "layout request is valid",
        "root_a_size_mb=0",
        "Use a positive size value.",
    );

    let payload = err.payload();
    assert_eq!(payload.code, "E003");
    assert_eq!(payload.component, "plan");
    assert!(!payload.expectation.is_empty());
    assert!(!payload.observed.is_empty());
    assert!(!payload.remediation.is_empty());
}

#[test]
fn exit_codes_remain_stable() {
    assert_eq!(ErrorCode::InvalidTargetDisk.exit_code(), 1);
    assert_eq!(ErrorCode::MissingTool.exit_code(), 2);
    assert_eq!(ErrorCode::PlanGeneration.exit_code(), 3);
    assert_eq!(ErrorCode::MissingConfirmation.exit_code(), 4);
}
