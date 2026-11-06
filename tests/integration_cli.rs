use recpart::types::ERROR_SCHEMA_VERSION;
use serde_json::Value;
use std::process::Command;

fn run_recpart(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_recpart"))
        .args(args)
        .output()
        .expect("the Cargo-built recpart binary should execute")
}

/// A target that is not a block device fails with the JSON error payload on
/// stderr and its code as the exit status.
#[test]
fn plan_json_error_contract_for_invalid_disk() {
    let output = run_recpart(&["plan", "--disk", "/dev/null", "--json"]);

    assert_eq!(output.status.code(), Some(1), "expected E001 exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let payload: Value = serde_json::from_str(&stderr)
        .unwrap_or_else(|err| panic!("invalid JSON: {err}\n---\n{stderr}\n---"));
    assert_eq!(payload["schema_version"], ERROR_SCHEMA_VERSION);
    assert_eq!(payload["code"], "E001");
    assert_eq!(payload["component"], "preflight");
    assert!(payload["expectation"]
        .as_str()
        .unwrap_or_default()
        .contains("block device"));
    assert!(!payload["remediation"]
        .as_str()
        .unwrap_or_default()
        .is_empty());
}
