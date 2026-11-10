//! A plan names the disk it was made for, `apply --expect` refuses a disk
//! that no longer gives the confirmed plan, and signatures on the bare disk
//! are listed as erased. Everything runs against recorded inventories
//! (`--inventory`), so no disk is read or written.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const DISK: &str = "/dev/vdq";
const MIB: u64 = 1024 * 1024;

fn run_recpart(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_recpart"))
        .args(args)
        .output()
        .expect("the Cargo-built recpart binary should execute")
}

fn stdout_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "recpart failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("recpart prints JSON on stdout")
}

fn error_json(output: &Output, code: &str) -> Value {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let payload: Value = serde_json::from_str(&stderr)
        .unwrap_or_else(|err| panic!("invalid JSON: {err}\n---\n{stderr}\n---"));
    assert_eq!(payload["code"], code, "{payload}");
    let status: i32 = code.trim_start_matches('E').parse().unwrap();
    assert_eq!(output.status.code(), Some(status));
    payload
}

/// A recorded 64 GiB GPT disk with one ext4 partition, written in the shape
/// of an older recpart (no `signatures` field).
fn gpt_inventory() -> Value {
    json!({
        "schema_version": 1,
        "disk": {
            "path": DISK,
            "size_bytes": 64 * 1024 * MIB,
            "logical_sector_bytes": 512,
            "physical_sector_bytes": 512,
            "model": "Test Disk",
            "vendor": "unknown",
            "serial": "SERIAL-A",
            "transport": "virtio",
            "rotational": false,
            "removable": false,
            "read_only": false
        },
        "table": "gpt",
        "table_id": "11111111-2222-3333-4444-555555555555",
        "usable_start_bytes": 17408,
        "usable_end_bytes": 64 * 1024 * MIB - 16896,
        "partitions": [{
            "number": 1,
            "device": format!("{DISK}1"),
            "start_bytes": MIB,
            "size_bytes": 10 * 1024 * MIB,
            "type_guid": "0FC63DAF-8483-4772-8E79-3D69D8477DE4",
            "label": null,
            "part_uuid": "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE",
            "attrs": null,
            "filesystem": "ext4",
            "fs_label": "data",
            "fs_uuid": null,
            "used_bytes": MIB,
            "mountpoints": [],
            "min_size_bytes": null,
            "max_size_bytes": null
        }],
        "free": []
    })
}

/// The same disk with no table and a LUKS container written to it whole.
fn luks_inventory() -> Value {
    let mut inventory = gpt_inventory();
    inventory["table"] = json!("none");
    inventory["table_id"] = Value::Null;
    inventory["partitions"] = json!([]);
    inventory["signatures"] = json!([{
        "kind": "crypto_LUKS",
        "offset_bytes": 0,
        "uuid": "0a1b2c3d-0000-4000-8000-000000000000",
        "label": null
    }]);
    inventory
}

fn write(dir: &Path, name: &str, value: &Value) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    path
}

#[test]
fn human_plan_explains_data_loss_sizes_and_preview() {
    let dir = tempfile::tempdir().unwrap();
    let original = gpt_inventory();
    let inventory = write(dir.path(), "inventory.json", &original);
    let output = run_recpart(&["plan", "--disk", DISK, "--inventory", inventory.to_str().unwrap()]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Preview only: no changes made."));
    assert!(text.contains("64.0 GiB"));
    assert!(text.contains("erases all existing partitions"));
    assert!(text.contains("10.0 GiB, ext4 -> absent"));
    assert!(text.contains("--dry-run"));
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(inventory).unwrap()).unwrap(), original);
}

fn plan(inventory: &Path, extra: &[&str]) -> Output {
    let mut args = vec!["plan", "--disk", DISK, "--inventory", inventory.to_str().unwrap()];
    args.extend(extra);
    args.push("--json");
    run_recpart(&args)
}

fn dry_run(inventory: &Path, expect: &Path) -> Output {
    run_recpart(&[
        "apply",
        "--disk",
        DISK,
        "--mode",
        "ab",
        "--inventory",
        inventory.to_str().unwrap(),
        "--expect",
        expect.to_str().unwrap(),
        "--dry-run",
        "--json",
    ])
}

#[test]
fn a_plan_names_its_disk_and_table() {
    let dir = tempfile::tempdir().unwrap();
    let inventory = write(dir.path(), "inventory.json", &gpt_inventory());
    let planned = stdout_json(&plan(&inventory, &["--mode", "ab"]));
    assert_eq!(planned["disk_serial"], "SERIAL-A");
    assert_eq!(planned["table_id"], "11111111-2222-3333-4444-555555555555");
    assert_eq!(planned["erased_signatures"], json!([]));
}

#[test]
fn apply_expect_refuses_a_changed_disk() {
    let dir = tempfile::tempdir().unwrap();
    let inventory = write(dir.path(), "inventory.json", &gpt_inventory());
    let planned = plan(&inventory, &["--mode", "ab"]);
    assert!(planned.status.success());
    let expect = dir.path().join("plan.json");
    std::fs::write(&expect, &planned.stdout).unwrap();

    // The same disk gives the same plan: the dry run proceeds.
    let same = stdout_json(&dry_run(&inventory, &expect));
    assert_eq!(same["dry_run"], true);

    // Another disk at the same path (a different serial).
    let mut other = gpt_inventory();
    other["disk"]["serial"] = json!("SERIAL-B");
    let other = write(dir.path(), "other.json", &other);
    let refused = error_json(&dry_run(&other, &expect), "E023");
    assert_eq!(refused["component"], "apply");
    assert!(refused["observed"].as_str().unwrap().contains("serial is SERIAL-B"), "{refused}");
    assert!(refused["remediation"].as_str().unwrap().contains("inspect it again"));

    // The same disk with its partition resized since.
    let mut resized = gpt_inventory();
    resized["partitions"][0]["size_bytes"] = json!(20 * 1024 * MIB);
    let resized = write(dir.path(), "resized.json", &resized);
    let refused = error_json(&dry_run(&resized, &expect), "E023");
    assert!(refused["observed"].as_str().unwrap().contains("summary"), "{refused}");

    // A new table on it.
    let mut retabled = gpt_inventory();
    retabled["table_id"] = json!("99999999-2222-3333-4444-555555555555");
    let retabled = write(dir.path(), "retabled.json", &retabled);
    let refused = error_json(&dry_run(&retabled, &expect), "E023");
    assert!(refused["observed"].as_str().unwrap().contains("table id"), "{refused}");
}

#[test]
fn apply_expect_ignores_formatting_but_not_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let inventory = write(dir.path(), "inventory.json", &gpt_inventory());
    let planned = stdout_json(&plan(&inventory, &["--mode", "ab"]));
    // Compact JSON, keys reordered by serde_json's map: still the same plan.
    let expect = dir.path().join("plan.json");
    std::fs::write(&expect, serde_json::to_string(&planned).unwrap()).unwrap();
    assert!(dry_run(&inventory, &expect).status.success());

    let mutable = run_recpart(&[
        "apply", "--disk", DISK, "--mode", "mutable", "--inventory", inventory.to_str().unwrap(),
        "--expect", expect.to_str().unwrap(), "--dry-run", "--json",
    ]);
    let refused = error_json(&mutable, "E023");
    assert!(refused["observed"].as_str().unwrap().contains("mode is mutable"), "{refused}");
    assert!(refused["remediation"].as_str().unwrap().contains("--mode"));
}

#[test]
fn apply_expect_rejects_a_file_that_is_not_a_plan() {
    let dir = tempfile::tempdir().unwrap();
    let inventory = write(dir.path(), "inventory.json", &gpt_inventory());
    let not_a_plan = write(dir.path(), "not-a-plan.json", &json!(["no"]));
    error_json(&dry_run(&inventory, &not_a_plan), "E027");
    let garbage = dir.path().join("garbage.json");
    std::fs::write(&garbage, "{ not json").unwrap();
    error_json(&dry_run(&inventory, &garbage), "E027");
    error_json(&dry_run(&inventory, &dir.path().join("missing.json")), "E027");
}

#[test]
fn signatures_on_the_bare_disk_are_listed_as_erased() {
    let dir = tempfile::tempdir().unwrap();
    let inventory = write(dir.path(), "luks.json", &luks_inventory());

    let inspected = stdout_json(&run_recpart(&[
        "inspect", "--disk", DISK, "--inventory", inventory.to_str().unwrap(), "--json",
    ]));
    assert_eq!(inspected["signatures"][0]["kind"], "crypto_LUKS");

    // A template erases the disk, the container with it.
    let planned = stdout_json(&plan(&inventory, &["--mode", "ab"]));
    assert_eq!(planned["wipes_disk"], true);
    assert_eq!(planned["erased_signatures"], inspected["signatures"]);

    // A layout must say new_table to erase it...
    let layout = json!({
        "actions": [
            { "action": "create", "role": "efi", "size_mb": 1024 },
            { "action": "create", "role": "root_a", "size_mb": 20480 },
            { "action": "create", "role": "root_b", "size_mb": 20480 },
            { "action": "create", "role": "state" }
        ]
    });
    let implicit = write(dir.path(), "implicit.json", &layout);
    let refused = plan(&inventory, &["--mode", "ab", "--layout", implicit.to_str().unwrap()]);
    let refused = error_json(&refused, "E014");
    assert!(refused["observed"].as_str().unwrap().contains("crypto_LUKS"), "{refused}");

    // ...and then lists it.
    let mut explicit = layout;
    explicit["new_table"] = json!(true);
    let explicit = write(dir.path(), "explicit.json", &explicit);
    let planned = stdout_json(&plan(&inventory, &["--mode", "ab", "--layout", explicit.to_str().unwrap()]));
    assert_eq!(planned["wipes_disk"], true);
    assert_eq!(planned["erased_signatures"][0]["kind"], "crypto_LUKS");
}
