use crate::plan::partition_device;
use crate::types::{
    HandoffPayload, InstallMode, ModeContext, MountMapping, PartitionPlan, HANDOFF_SCHEMA_VERSION,
};

pub fn build_handoff(plan: &PartitionPlan, mounted: Vec<MountMapping>) -> HandoffPayload {
    // The install target is where the partition planned for "/" is mounted.
    let root = plan.partitions.iter().find(|p| p.mountpoint == "/");
    let install_target = root
        .and_then(|p| {
            let device = partition_device(&plan.disk.path, p.index);
            mounted.iter().find(|m| m.device == device)
        })
        .map(|m| m.path.clone())
        .unwrap_or_default();

    let mut next_commands = vec![
        format!("recstrap {install_target}"),
        format!("recfstab {install_target} >> {install_target}/etc/fstab"),
        format!("recchroot {install_target}"),
    ];

    let mode_context = match plan.mode {
        InstallMode::Ab => {
            let slot = |name: &str| plan.partitions.iter().find(|p| p.name == name);
            let device_of = |name: &str| slot(name).map(|p| partition_device(&plan.disk.path, p.index));
            // The slot installed now is the one mounted at "/"; the other is
            // left unmounted for later updates.
            let target_slot = |name: &str| slot(name).is_some_and(|p| p.mountpoint == "/");
            let (install_target_slot, inactive_slot_hint) = if target_slot("root_a") {
                ("A", "B")
            } else {
                ("B", "A")
            };
            next_commands.push("recab status".to_string());

            ModeContext {
                install_target_slot: Some(install_target_slot.to_string()),
                inactive_slot_hint: Some(inactive_slot_hint.to_string()),
                slot_a_device: device_of("root_a"),
                slot_b_device: device_of("root_b"),
                notes: vec![
                    format!("A/B mode installs to slot {install_target_slot}."),
                    "After validation, choose the next boot slot with recab set-next <A|B>."
                        .to_string(),
                ],
            }
        }
        InstallMode::Mutable => ModeContext {
            install_target_slot: None,
            inactive_slot_hint: None,
            slot_a_device: None,
            slot_b_device: None,
            notes: vec!["Mutable mode uses a single writable root partition.".to_string()],
        },
    };

    HandoffPayload {
        schema_version: HANDOFF_SCHEMA_VERSION,
        mode: plan.mode,
        install_target,
        mount_map: mounted,
        next_commands,
        mode_context,
    }
}
