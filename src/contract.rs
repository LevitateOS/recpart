//! recpart's contract for `cargo xtask gen`: the JSON it prints, its protocol
//! constants and the facts front ends preview (sizes, roles, error codes).

use anyhow::Result;
use std::collections::BTreeMap;
use tool_contract::Contract;

use crate::error::ErrorCode;
use crate::policy::{self, template_roles};
use crate::types::{self, Filesystem, InstallMode, Role};

pub fn export() -> Result<Contract> {
    let schemas = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/frontend-handoff/schemas");
    let mut c = Contract::new("recpart", &["recpart"]);
    c.schema::<types::DiskListResult>(schemas, "list-disks")?
        .schema::<types::InspectResult>(schemas, "inspect")?
        .schema::<types::ManualLayout>(schemas, "layout")?
        .schema::<types::PartitionPlan>(schemas, "plan-result")?
        .schema::<types::ApplyResult>(schemas, "apply-result")?
        .schema::<types::ErrorPayload>(schemas, "error")?;
    c.types_from::<types::DiskListResult>()
        .types_from::<types::InspectResult>()
        .types_from::<types::ManualLayout>()
        .types_from::<types::PartitionPlan>()
        .types_from::<types::ApplyResult>()
        .types_from::<types::ErrorPayload>();
    c.constant("PLAN_SCHEMA_VERSION", "", &types::PLAN_SCHEMA_VERSION)?
        .constant("APPLY_SCHEMA_VERSION", "", &types::APPLY_SCHEMA_VERSION)?
        .constant("ERROR_SCHEMA_VERSION", "", &types::ERROR_SCHEMA_VERSION)?
        .constant("DISK_LIST_SCHEMA_VERSION", "", &types::DISK_LIST_SCHEMA_VERSION)?
        .constant("INSPECT_SCHEMA_VERSION", "", &types::INSPECT_SCHEMA_VERSION)?
        .constant("CONFIRM_TOKEN", "The `--confirm` value a destructive apply requires.", &types::CONFIRM_TOKEN)?;
    c.value_enum::<InstallMode>("INSTALL_MODES", "INSTALL_MODE_HELP")?
        .constant("DEFAULT_INSTALL_MODE", "recpart's default mode (`--mode` unset).", &InstallMode::default())?
        .value_enum::<Role>("ROLES", "ROLE_HELP")?
        .constant("FILESYSTEMS", "The filesystems recpart makes, by blkid name.", Filesystem::ALL)?;

    let roles: BTreeMap<String, serde_json::Value> = Role::value_variants_of()
        .iter()
        .map(|role| {
            let fact = serde_json::json!({
                "filesystem": role.filesystem(),
                "label": role.label(),
                "mountpoint": role.mountpoint(),
            });
            (serde_json::to_value(role).unwrap().as_str().unwrap().to_string(), fact)
        })
        .collect();
    c.constant("ROLE_FACTS", "Each role's filesystem, label and installed mount point (null: not mounted, or for `other` the layout's own).", &roles)?;
    let modes: BTreeMap<String, &[Role]> = InstallMode::value_variants_of()
        .iter()
        .map(|mode| (mode.to_string(), template_roles(*mode)))
        .collect();
    c.constant("TEMPLATE_ROLES", "The roles each mode's template creates, in order (the roles a manual layout of that mode needs).", &modes)?;
    c.constant(
        "SIZE_POLICY",
        "recpart's size policy, in MiB.",
        &serde_json::json!({
            "defaultEfiMb": policy::DEFAULT_EFI_SIZE_MB,
            "defaultAbRootMb": policy::DEFAULT_AB_ROOT_SIZE_MB,
            "minAbStateMb": policy::DEFAULT_AB_MIN_STATE_MB,
            "minMutableRootMb": policy::DEFAULT_MUTABLE_MIN_ROOT_MB,
            "minAbRootMb": policy::MIN_AB_ROOT_MB,
            "minNewEfiMb": policy::MIN_NEW_EFI_MB,
            "minSwapMb": policy::MIN_SWAP_MB,
            "efiBootFilesMb": InstallMode::value_variants_of().iter().map(|m| (m.to_string(), policy::efi_boot_files_mb(*m))).collect::<BTreeMap<_, _>>(),
            "minDiskMb": InstallMode::value_variants_of().iter().map(|m| (m.to_string(), policy::min_disk_mb(*m))).collect::<BTreeMap<_, _>>(),
        }),
    )?;
    c.constant("EFI_TYPE", "The GPT type GUID of an EFI system partition.", policy::EFI_TYPE)?;
    let codes: BTreeMap<String, String> = ErrorCode::ALL.iter().map(|code| (code.name(), code.to_string())).collect();
    c.constant("ERROR_CODES", "recpart's error codes by name; a code's number is recpart's exit status.", &codes)?;
    c.raw(include_str!("contract/output.ts"));
    Ok(c)
}

trait Variants: Sized + 'static {
    fn value_variants_of() -> &'static [Self];
}

impl<T: clap::ValueEnum + 'static> Variants for T {
    fn value_variants_of() -> &'static [Self] {
        T::value_variants()
    }
}
