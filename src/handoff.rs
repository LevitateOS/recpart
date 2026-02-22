use crate::plan::partition_device;
use crate::policy::DEFAULT_AB_TARGET_SLOT;
use crate::types::{
    HandoffPayload, InstallMode, ModeContext, MountMapping, PartitionPlan, HANDOFF_SCHEMA_VERSION,
};

pub fn build_handoff(plan: &PartitionPlan, mounted: Vec<MountMapping>) -> HandoffPayload {
    let install_target = mounted
        .iter()
        .find(|m| m.path.ends_with("/sysroot"))
        .map(|m| m.path.clone())
        .unwrap_or_else(|| "/mnt/sysroot".to_string());

    let mut next_commands = vec![
        format!("recstrap {install_target}"),
        format!("recfstab {install_target} >> {install_target}/etc/fstab"),
        format!("recchroot {install_target}"),
    ];

    let mode_context = match plan.mode {
        InstallMode::Ab => {
            let slot_a_device = Some(partition_device(&plan.disk.path, 2));
            let slot_b_device = Some(partition_device(&plan.disk.path, 3));
            let install_target_slot = DEFAULT_AB_TARGET_SLOT.to_string();
            let inactive_slot_hint = if install_target_slot == "A" {
                "B".to_string()
            } else {
                "A".to_string()
            };
            next_commands.push("recab status".to_string());
            next_commands.push(
                "# after validation, choose slot policy with recab set-next <A|B>".to_string(),
            );

            ModeContext {
                install_target_slot: Some(install_target_slot),
                inactive_slot_hint: Some(inactive_slot_hint),
                slot_a_device,
                slot_b_device,
                notes: vec![
                    "A/B mode defaults install target to slot A for first install run.".to_string(),
                    "Use recab to manage active/inactive slot transitions after installation."
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
