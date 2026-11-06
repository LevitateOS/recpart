use crate::error::{ErrorCode, RecpartError, Result};
use crate::policy::{build_templates, remaining_min_mb, resolve_layout_request};
use crate::types::{
    DiskTarget, InstallMode, LayoutRequest, PartitionPlan, PartitionTemplate, PLAN_SCHEMA_VERSION,
};
use std::path::Path;

const MB: u64 = 1024 * 1024;

/// Plan the layout for `disk`; the disk must hold every fixed-size partition
/// plus the policy minimum for the one that takes the remaining space.
pub fn build_plan(
    disk: DiskTarget,
    mode: InstallMode,
    layout_request: LayoutRequest,
) -> Result<PartitionPlan> {
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

    Ok(PartitionPlan {
        schema_version: PLAN_SCHEMA_VERSION,
        mode,
        layout_request,
        resolved_layout,
        disk,
        partitions,
        sfdisk_script,
    })
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

pub fn partition_device(disk: &Path, index: u8) -> String {
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
