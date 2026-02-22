use serde_json::Value;
use std::process::Command;

fn run_recpart(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "-q", "-p", "recpart", "--"])
        .args(args)
        .output()
        .expect("recpart command should execute through cargo run")
}

fn parse_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|err| panic!("invalid JSON: {err}\n---\n{s}\n---"))
}

#[test]
fn help_shows_expected_commands() {
    let output = run_recpart(&["--help"]);

    assert!(output.status.success(), "help failed: {:?}", output.status);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list-disks"), "help missing list-disks command");
    assert!(stdout.contains("plan"), "help missing plan command");
    assert!(stdout.contains("apply"), "help missing apply command");
    assert!(
        !stdout.contains(" tui"),
        "help still includes removed tui command"
    );
}

#[test]
fn plan_json_error_contract_for_invalid_disk() {
    let output = run_recpart(&["plan", "--disk", "/dev/null", "--json"]);

    assert_eq!(output.status.code(), Some(1), "expected E001 exit code");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let payload = parse_json(&stderr);

    assert_eq!(payload["schema_version"], 1);
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

#[test]
fn list_disks_json_contract_has_schema_and_array() {
    let output = run_recpart(&["list-disks", "--json"]);

    assert!(
        output.status.success(),
        "list-disks --json failed: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload = parse_json(&stdout);
    assert_eq!(payload["schema_version"], 1);
    assert!(payload["disks"].is_array());
}
