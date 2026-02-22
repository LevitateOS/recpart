use crate::error::{ErrorCode, RecpartError, Result};
use crate::policy::{build_templates, required_min_mb, resolve_layout_request};
use crate::types::{
    DiskTarget, InstallMode, LayoutRequest, PartitionPlan, PartitionTemplate, PLAN_SCHEMA_VERSION,
};
use std::path::Path;

const MB: u64 = 1024 * 1024;

pub fn build_plan(disk: DiskTarget, mode: InstallMode) -> Result<PartitionPlan> {
    build_plan_with_layout_request(disk, mode, LayoutRequest::default())
}

pub fn build_plan_with_layout_request(
    disk: DiskTarget,
    mode: InstallMode,
    layout_request: LayoutRequest,
) -> Result<PartitionPlan> {
    let resolved_layout = resolve_layout_request(mode, &layout_request)?;
    let disk_mb = disk.size_bytes / MB;
    let min_required = u64::from(required_min_mb(&resolved_layout));

    if disk_mb < min_required {
        return Err(RecpartError::new(
            ErrorCode::PlanGeneration,
            "plan",
            format!("disk must have at least {min_required}MB for mode {mode}"),
            format!("disk has {disk_mb}MB"),
            "Choose a larger disk or adjust partition policy defaults.",
        ));
    }

    let partitions = build_templates(&resolved_layout);
    validate_partition_fit(&partitions, disk_mb, mode)?;
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
        let line = match (part.size_mb, part.index == 1 && part.gpt_type == "U") {
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

fn validate_partition_fit(
    partitions: &[PartitionTemplate],
    disk_mb: u64,
    mode: InstallMode,
) -> Result<()> {
    let fixed_mb = partitions
        .iter()
        .filter_map(|p| p.size_mb.map(u64::from))
        .sum::<u64>();
    let has_remaining = partitions.iter().any(|p| p.size_mb.is_none());

    if fixed_mb > disk_mb {
        return Err(RecpartError::new(
            ErrorCode::PlanGeneration,
            "plan",
            "sum of fixed partition sizes does not exceed disk size",
            format!("fixed partition sum is {fixed_mb}MB, disk is {disk_mb}MB"),
            "Reduce fixed partition sizes or select a larger disk.",
        ));
    }

    if has_remaining && fixed_mb >= disk_mb {
        return Err(RecpartError::new(
            ErrorCode::PlanGeneration,
            "plan",
            "at least one MB remains for remaining-size partition",
            format!("no free space left for remaining partition in mode {mode}"),
            "Reduce fixed partition sizes or set explicit sizes for all partitions.",
        ));
    }

    Ok(())
}
