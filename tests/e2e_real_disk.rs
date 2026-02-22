use serde_json::Value;
use std::env;
use std::path::Path;
use std::process::Command;

fn run_recpart(args: &[&str]) -> std::process::Output {
    Command::new("cargo")
        .args(["run", "-q", "-p", "recpart", "--"])
        .args(args)
        .output()
        .expect("recpart command should execute through cargo run")
}

fn require_e2e_disk() -> Option<String> {
    env::var("RECPART_E2E_DISK")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn parse_json(s: &str) -> Value {
    serde_json::from_str(s).unwrap_or_else(|err| panic!("invalid JSON: {err}\n---\n{s}\n---"))
}

#[test]
#[ignore = "requires real throwaway disk via RECPART_E2E_DISK"]
fn e2e_plan_on_real_disk_json_contract() {
    let Some(disk) = require_e2e_disk() else {
        eprintln!("skip: set RECPART_E2E_DISK=/dev/<disk>");
        return;
    };

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
    let Some(disk) = require_e2e_disk() else {
        eprintln!("skip: set RECPART_E2E_DISK=/dev/<disk>");
        return;
    };

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
    let Some(disk) = require_e2e_disk() else {
        eprintln!("skip: set RECPART_E2E_DISK=/dev/<disk>");
        return;
    };

    if env::var("RECPART_E2E_ALLOW_DESTRUCTIVE").ok().as_deref() != Some("YES_DESTROY") {
        eprintln!("skip: set RECPART_E2E_ALLOW_DESTRUCTIVE=YES_DESTROY to run destructive e2e");
        return;
    }

    let uid_output = Command::new("id")
        .arg("-u")
        .output()
        .expect("id -u must execute");
    let uid = String::from_utf8_lossy(&uid_output.stdout)
        .trim()
        .to_string();
    if uid != "0" {
        eprintln!("skip: destructive e2e requires root");
        return;
    }

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
        let _ = Command::new("umount").arg(&target).status();
    }

    for dir in [
        Path::new(mount_root).join("sysroot/boot"),
        Path::new(mount_root).join("sysroot/state"),
        Path::new(mount_root).join("sysroot"),
        Path::new(mount_root).to_path_buf(),
    ] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
