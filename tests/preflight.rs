use distro_spec::shared::error::ToolErrorCode;
use recpart::preflight::{query_disk_target, tool_in_path};
use std::path::Path;

#[test]
fn tool_in_path_returns_false_for_missing_binary() {
    assert!(!tool_in_path("recpart-definitely-missing-tool-xyz"));
}

#[test]
fn invalid_disk_path_returns_e001() {
    let err = query_disk_target(Path::new("/dev/null")).expect_err("must fail");
    assert_eq!(err.code.code(), "E001");
}
