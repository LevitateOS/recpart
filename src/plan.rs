use crate::error::{ErrorCode, RecpartError, Result};
use crate::policy::{build_templates, remaining_min_mb, resolve_layout_request};
use crate::manual::state_of;
use crate::types::{
    ChangeKind, InspectResult, InstallMode, LayoutRequest, PartitionChange, PartitionPlan,
    PartitionState, PartitionTemplate, PLAN_SCHEMA_VERSION,
};
use std::path::Path;

const MB: u64 = 1024 * 1024;

/// Plan the layout for `disk`; the disk must hold every fixed-size partition
/// plus the policy minimum for the one that takes the remaining space.
pub fn build_plan(
    inventory: &InspectResult,
    mode: InstallMode,
    layout_request: LayoutRequest,
) -> Result<PartitionPlan> {
    let disk = inventory.disk.target.clone();
    let resolved_layout = resolve_layout_request(mode, &layout_request)?;
    let disk_mb = disk.size_bytes / MB;
    let partitions = build_templates(&resolved_layout);
    let min_required: u64 = partitions
        .iter()
        .map(|p| u64::from(p.size_mb.unwrap_or_else(|| remaining_min_mb(mode))))
        .sum();

    if disk_mb < min_required {
        return Err(RecpartError::new(
            ErrorCode::PlanGeneration,
            "plan",
            format!("disk must have at least {min_required}MB for mode {mode}"),
            format!("disk has {disk_mb}MB"),
            "Choose a larger disk or adjust partition policy defaults.",
        ));
    }

    let sfdisk_script = build_sfdisk_script(&partitions);
    let summary = template_summary(inventory, &partitions);

    Ok(PartitionPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        mode,
        layout_request,
        resolved_layout,
        disk,
        partitions,
        sfdisk_script,
        manual_layout: None,
        wipes_disk: true,
        summary,
        swap_file: None,
        disk_serial: inventory.disk.serial.clone(),
        table_id: inventory.table_id.clone(),
        erased_signatures: inventory.signatures.clone(),
    })
}

/// A template erases every partition on the disk and creates its own, one
/// after another from 1 MiB, the last taking the rest (as sfdisk places them).
fn template_summary(inventory: &InspectResult, partitions: &[PartitionTemplate]) -> Vec<PartitionChange> {
    let sector = u64::from(inventory.disk.target.logical_sector_bytes);
    let (mut start, end) = crate::inspect::new_gpt_usable(inventory.disk.target.size_bytes, sector);
    let erased = inventory.partitions.iter().map(|part| PartitionChange {
        number: part.number,
        device: part.device.clone(),
        change: ChangeKind::Erased,
        before: Some(state_of(part)),
        after: None,
        role: None,
        mountpoint: None,
    });
    let mut created = Vec::new();
    for part in partitions {
        let size = part
            .size_mb
            .map(|mb| u64::from(mb) * MB)
            // sfdisk ends the last partition on a 1 MiB boundary.
            .unwrap_or_else(|| crate::inspect::align_down(end, MB).saturating_sub(start));
        created.push(PartitionChange {
            number: u32::from(part.index),
            device: partition_device(&inventory.disk.target.path, part.index.into()),
            change: ChangeKind::Created,
            before: None,
            after: Some(PartitionState {
                start_bytes: start,
                size_bytes: size,
                filesystem: Some(part.filesystem.as_str().to_string()),
                fs_label: Some(part.label.clone()),
            }),
            role: crate::types::Role::from_template_name(&part.name),
            mountpoint: (part.mountpoint != crate::policy::NOT_MOUNTED).then(|| part.mountpoint.clone()),
        });
        start += size;
    }
    erased.chain(created).collect()
}

pub fn build_sfdisk_script(partitions: &[PartitionTemplate]) -> String {
    let mut lines = Vec::with_capacity(partitions.len() + 1);
    lines.push("label: gpt".to_string());

    for part in partitions {
        let line = match (part.size_mb, part.bootable) {
            (Some(size_mb), true) => format!(",{size_mb}M,{},*", part.gpt_type),
            (Some(size_mb), false) => format!(",{size_mb}M,{}", part.gpt_type),
            (None, _) => format!(",,{}", part.gpt_type),
        };
        lines.push(line);
    }

    let mut script = lines.join("\n");
    script.push('\n');
    script
}

pub fn partition_device(disk: &Path, index: u32) -> String {
    let base = disk.to_string_lossy();
    let needs_p = base
        .chars()
        .last()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false);

    if needs_p {
        format!("{base}p{index}")
    } else {
        format!("{base}{index}")
    }
}
