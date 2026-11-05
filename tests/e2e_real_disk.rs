use serde_json::Value;
use std::env;
use std::path::Path;
use std::process::Command;

fn run_recpart(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_recpart"))
        .args(args)
        .output()
        .expect("the Cargo-built recpart binary should execute")
}

fn require_e2e_disk() -> String {
    let disk = env::var("RECPART_E2E_DISK").expect(
        "recpart real-disk e2e: set RECPART_E2E_DISK=/dev/<throwaway-disk> before explicitly running --ignored tests",
    );
    assert!(
        !disk.trim().is_empty(),
        "RECPART_E2E_DISK must name a throwaway disk"
    );
    disk
}

fn parse_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|err| panic!("invalid JSON: {err}\n---\n{s}\n---"))
}

#[test]
#[ignore = "requires real throwaway disk via RECPART_E2E_DISK"]
fn e2e_plan_on_real_disk_json_contract() {
    let disk = require_e2e_disk();

    let output = run_recpart(&["plan", "--disk", &disk, "--mode", "ab", "--json"]);

    assert!(output.status.success(), "plan failed: {:?}", output.status);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload = parse_json(&stdout);

    assert_eq!(payload["schema_version"], 1);
    assert_eq!(payload["mode"], "ab");
    assert_eq!(payload["disk"]["path"], disk);
    assert_eq!(payload["partitions"].as_array().map(|v| v.len()), Some(4));
}

#[test]
#[ignore = "requires real throwaway disk via RECPART_E2E_DISK"]
fn e2e_apply_dry_run_on_real_disk_json_contract() {
    let disk = require_e2e_disk();

    let mount_root = "/mnt/recpart-e2e";
    let output = run_recpart(&[
        "apply",
        "--disk",
        &disk,
        "--mode",
        "ab",
        "--dry-run",
        "--mount-root",
        mount_root,
        "--json",
    ]);

    assert!(
        output.status.success(),
        "apply dry-run failed: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload = parse_json(&stdout);

    assert_eq!(payload["schema_version"], 1);
    assert_eq!(payload["dry_run"], true);
    assert_eq!(payload["mode"], "ab");
    assert_eq!(payload["handoff"]["schema_version"], 1);
    assert!(
        payload["steps"]
            .as_array()
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        "dry-run should include step list"
    );
}

#[test]
#[ignore = "destructive: requires throwaway disk + root + RECPART_E2E_ALLOW_DESTRUCTIVE=YES_DESTROY"]
fn e2e_apply_destructive_ab_mode() {
    let disk = require_e2e_disk();

    assert_eq!(env::var("RECPART_E2E_ALLOW_DESTRUCTIVE").ok().as_deref(), Some("YES_DESTROY"),
        "recpart destructive e2e: set RECPART_E2E_ALLOW_DESTRUCTIVE=YES_DESTROY for a designated throwaway disk");

    let uid_output = Command::new("id")
        .arg("-u")
        .output()
        .expect("id -u must execute");
    assert!(
        uid_output.status.success(),
        "id -u failed: {}",
        String::from_utf8_lossy(&uid_output.stderr)
    );
    let uid = String::from_utf8_lossy(&uid_output.stdout)
        .trim()
        .to_string();
    assert_eq!(
        uid, "0",
        "recpart destructive e2e requires root; explicitly rerun the acknowledged test with sudo"
    );

    let mount_root = "/mnt/recpart-e2e-live";
    let output = run_recpart(&[
        "apply",
        "--disk",
        &disk,
        "--mode",
        "ab",
        "--mount-root",
        mount_root,
        "--confirm",
        "DESTROY",
        "--json",
    ]);

    if !output.status.success() {
        panic!(
            "destructive apply failed: {:?}\nstdout: {}\nstderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload = parse_json(&stdout);
    assert_eq!(payload["dry_run"], false);

    for target in [
        format!("{mount_root}/sysroot"),
        format!("{mount_root}/sysroot/boot"),
        format!("{mount_root}/sysroot/state"),
    ] {
        let status = Command::new("findmnt")
            .args(["-rno", "TARGET", &target])
            .status()
            .expect("findmnt should execute");
        assert!(status.success(), "expected mounted target: {target}");
    }

    for target in [
        format!("{mount_root}/sysroot/boot"),
        format!("{mount_root}/sysroot/state"),
        format!("{mount_root}/sysroot"),
    ] {
        let output = Command::new("umount")
            .arg(&target)
            .output()
            .expect("umount should execute");
        assert!(
            output.status.success(),
            "failed to unmount {target}; preserving mount directories: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for dir in [
        Path::new(mount_root).join("sysroot/boot"),
        Path::new(mount_root).join("sysroot/state"),
        Path::new(mount_root).join("sysroot"),
        Path::new(mount_root).to_path_buf(),
    ] {
        std::fs::remove_dir(&dir).unwrap_or_else(|err| {
            panic!(
                "cannot remove unmounted test directory {}: {err}",
                dir.display()
            )
        });
    }
}
