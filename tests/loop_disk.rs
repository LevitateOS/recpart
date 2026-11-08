//! recpart against a loop device backed by a sparse file: plan, dry-run and
//! a real apply, checked through the kernel's view of the disk. Needs root
//! (losetup, mkfs, mount); run with `--include-ignored` as root.

use recpart::types::{ApplyResult, InstallMode, PartitionPlan, CONFIRM_TOKEN};
use std::process::Command;

fn run(program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("{program}: {e}"));
    assert!(
        output.status.success(),
        "{program} {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn recpart(args: &[&str]) -> String {
    run(env!("CARGO_BIN_EXE_recpart"), args)
}

/// A partition-scanned loop device over a sparse image, detached (and
/// unmounted under `mount_root`) when dropped.
struct LoopDisk {
    device: String,
    mount_root: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl LoopDisk {
    fn new(size: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let image = dir.path().join("disk.img");
        run("truncate", &["-s", size, image.to_str().unwrap()]);
        let device = run("losetup", &["-P", "-f", "--show", image.to_str().unwrap()])
            .trim()
            .to_string();
        let mount_root = dir.path().join("mnt");
        Self {
            device,
            mount_root,
            _dir: dir,
        }
    }
}

fn report(what: &str, status: std::io::Result<std::process::ExitStatus>) {
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("cleanup: {what} failed ({status})"),
        Err(err) => eprintln!("cleanup: {what} did not run: {err}"),
    }
}

impl Drop for LoopDisk {
    fn drop(&mut self) {
        // The test may have unmounted it already; what does not clean up is said.
        let mounted = target_root::mount_table()
            .map(|table| table.iter().any(|m| m.mount_point.starts_with(&self.mount_root)));
        if mounted.unwrap_or(true) {
            report("umount -R", Command::new("umount").arg("-R").arg(&self.mount_root).status());
        }
        report("losetup -d", Command::new("losetup").args(["-d", &self.device]).status());
    }
}

#[test]
#[ignore = "needs root: creates a loop device, formats and mounts it"]
fn ab_layout_is_planned_applied_and_handed_off() {
    let disk = LoopDisk::new("50G");
    let mount_root = disk.mount_root.to_str().unwrap();

    let plan: PartitionPlan =
        serde_json::from_str(&recpart(&["plan", "--disk", &disk.device, "--mode", "ab", "--json"]))
            .unwrap();
    let roles = recpart::policy::template_roles(InstallMode::Ab);
    let layout: Vec<(&str, &str, &str)> = plan
        .partitions
        .iter()
        .map(|p| (p.label.as_str(), p.filesystem.as_str(), p.mountpoint.as_str()))
        .collect();
    let expected: Vec<(&str, &str, &str)> = roles
        .iter()
        .map(|r| {
            (r.label().unwrap(), r.filesystem().as_str(), r.mountpoint().unwrap_or(recpart::policy::NOT_MOUNTED))
        })
        .collect();
    assert_eq!(layout, expected);

    let dry: ApplyResult = serde_json::from_str(&recpart(&[
        "apply", "--disk", &disk.device, "--mode", "ab", "--dry-run", "--mount-root", mount_root,
        "--json",
    ]))
    .unwrap();
    assert!(dry.warnings.is_empty(), "apply would refuse: {:?}", dry.warnings);
    assert!(dry.steps.iter().all(|s| s.dry_run && s.status.is_none()));

    let applied: ApplyResult = serde_json::from_str(&recpart(&[
        "apply", "--disk", &disk.device, "--mode", "ab", "--confirm", CONFIRM_TOKEN,
        "--mount-root", mount_root, "--json",
    ]))
    .unwrap();
    assert!(applied.steps.iter().all(|s| s.status == Some(0)));

    // The kernel sees the planned labels and filesystems...
    let lsblk = run("lsblk", &["-nr", "-o", "LABEL,FSTYPE", &disk.device]);
    let seen: Vec<&str> = lsblk.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let expected: Vec<String> =
        roles.iter().map(|r| format!("{} {}", r.label().unwrap(), r.filesystem().as_str())).collect();
    assert_eq!(seen, expected);

    // ...and the handoff names what is really mounted, slot B left unmounted.
    assert_eq!(applied.handoff.install_target, mount_root);
    for mount in &applied.handoff.mount_map {
        let source = run("findmnt", &["-nr", "-o", "SOURCE", "--mountpoint", &mount.path]);
        assert_eq!(source.trim(), mount.device, "{} is not mounted from {}", mount.path, mount.device);
    }
    let mounted: Vec<&str> = applied.handoff.mount_map.iter().map(|m| m.path.as_str()).collect();
    assert_eq!(mounted.len(), 3, "{mounted:?}");
    let slots = &applied.handoff.mode_context;
    assert_eq!(slots.install_target_slot.as_deref(), Some("A"));
    assert!(!applied
        .handoff
        .mount_map
        .iter()
        .any(|m| Some(&m.device) == slots.slot_b_device.as_ref()));
}
