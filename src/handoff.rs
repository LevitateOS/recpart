use crate::error::{ErrorCode, RecpartError, Result};
use crate::plan::partition_device;
use crate::policy::ROOT_MOUNT;
use crate::types::{
    HandoffPayload, InstallMode, ModeContext, MountMapping, PartitionPlan, Role,
    HANDOFF_SCHEMA_VERSION,
};

pub fn build_handoff(plan: &PartitionPlan, mounted: Vec<MountMapping>, swaps: Vec<String>) -> Result<HandoffPayload> {
    // The install target is where the partition planned for "/" is mounted.
    let root = plan.partitions.iter().find(|p| p.mountpoint == ROOT_MOUNT);
    let install_target = root
        .and_then(|p| {
            let device = partition_device(&plan.disk.path, p.index.into());
            mounted.iter().find(|m| m.device == device)
        })
        .map(|m| m.path.clone())
        .ok_or_else(|| {
            RecpartError::new(
                ErrorCode::PlanGeneration,
                "plan",
                "the layout mounts a partition at / (the install target)",
                "no partition of the plan is mounted at /",
                "Give one partition the root role (root, or root_a for A/B).",
            )
        })?;

    let mode_context = match plan.mode {
        InstallMode::Ab => {
            let slot = |name: &str| plan.partitions.iter().find(|p| p.name == name);
            let device_of = |name: &str| slot(name).map(|p| partition_device(&plan.disk.path, p.index.into()));
            // The slot installed now is the one mounted at "/"; the other is
            // left unmounted for later updates.
            let target_slot = |name: &str| slot(name).is_some_and(|p| p.mountpoint == ROOT_MOUNT);
            let (install_target_slot, inactive_slot_hint) = if target_slot(Role::RootA.as_str()) {
                ("A", "B")
            } else {
                ("B", "A")
            };
            ModeContext {
                install_target_slot: Some(install_target_slot.to_string()),
                inactive_slot_hint: Some(inactive_slot_hint.to_string()),
                slot_a_device: device_of(Role::RootA.as_str()),
                slot_b_device: device_of(Role::RootB.as_str()),
            }
        }
        InstallMode::Mutable => ModeContext {
            install_target_slot: None,
            inactive_slot_hint: None,
            slot_a_device: None,
            slot_b_device: None,
        },
    };

    Ok(HandoffPayload {
        schema_version: HANDOFF_SCHEMA_VERSION,
        mode: plan.mode,
        install_target,
        mount_map: mounted,
        mode_context,
        swaps,
    })
}
