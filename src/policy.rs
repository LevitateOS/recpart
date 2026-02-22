use crate::error::{ErrorCode, RecpartError, Result};
use crate::types::{InstallMode, LayoutRequest, PartitionTemplate, ResolvedLayout};

pub const DEFAULT_AB_TARGET_SLOT: &str = "A";
pub const DEFAULT_EFI_SIZE_MB: u32 = 1024;
pub const DEFAULT_AB_ROOT_SIZE_MB: u32 = 20 * 1024;
pub const DEFAULT_AB_MIN_STATE_MB: u32 = 4 * 1024;
pub const DEFAULT_MUTABLE_MIN_ROOT_MB: u32 = 8 * 1024;

pub fn policy_defaults(mode: InstallMode) -> ResolvedLayout {
    match mode {
        InstallMode::Ab => ResolvedLayout {
            mode,
            efi_size_mb: DEFAULT_EFI_SIZE_MB,
            root_size_mb: None,
            root_a_size_mb: Some(DEFAULT_AB_ROOT_SIZE_MB),
            root_b_size_mb: Some(DEFAULT_AB_ROOT_SIZE_MB),
            state_size_mb: None,
        },
        InstallMode::Mutable => ResolvedLayout {
            mode,
            efi_size_mb: DEFAULT_EFI_SIZE_MB,
            root_size_mb: None,
            root_a_size_mb: None,
            root_b_size_mb: None,
            state_size_mb: None,
        },
    }
}

pub fn resolve_layout_request(
    mode: InstallMode,
    request: &LayoutRequest,
) -> Result<ResolvedLayout> {
    validate_request_fields(mode, request)?;
    let defaults = policy_defaults(mode);

    let mut resolved = defaults;
    if let Some(efi) = request.efi_size_mb {
        resolved.efi_size_mb = validate_positive("efi_size_mb", efi)?;
    }

    match mode {
        InstallMode::Ab => {
            if let Some(root_a) = request.root_a_size_mb {
                resolved.root_a_size_mb = Some(validate_positive("root_a_size_mb", root_a)?);
            }
            if let Some(root_b) = request.root_b_size_mb {
                resolved.root_b_size_mb = Some(validate_positive("root_b_size_mb", root_b)?);
            }
            if let Some(state) = request.state_size_mb {
                let state = validate_positive("state_size_mb", state)?;
                if state < DEFAULT_AB_MIN_STATE_MB {
                    return Err(RecpartError::new(
                        ErrorCode::PlanGeneration,
                        "policy",
                        format!("state_size_mb is at least {}", DEFAULT_AB_MIN_STATE_MB),
                        format!("state_size_mb={state}"),
                        format!(
                            "Use --state-size-mb >= {} for ab mode.",
                            DEFAULT_AB_MIN_STATE_MB
                        ),
                    ));
                }
                resolved.state_size_mb = Some(state);
            }
        }
        InstallMode::Mutable => {
            if let Some(root) = request.root_size_mb {
                let root = validate_positive("root_size_mb", root)?;
                if root < DEFAULT_MUTABLE_MIN_ROOT_MB {
                    return Err(RecpartError::new(
                        ErrorCode::PlanGeneration,
                        "policy",
                        format!("root_size_mb is at least {}", DEFAULT_MUTABLE_MIN_ROOT_MB),
                        format!("root_size_mb={root}"),
                        format!(
                            "Use --root-size-mb >= {} for mutable mode.",
                            DEFAULT_MUTABLE_MIN_ROOT_MB
                        ),
                    ));
                }
                resolved.root_size_mb = Some(root);
            }
        }
    }

    Ok(resolved)
}

pub fn required_min_mb(layout: &ResolvedLayout) -> u32 {
    match layout.mode {
        InstallMode::Ab => {
            let root_a = layout.root_a_size_mb.unwrap_or(DEFAULT_AB_ROOT_SIZE_MB);
            let root_b = layout.root_b_size_mb.unwrap_or(DEFAULT_AB_ROOT_SIZE_MB);
            let state = layout.state_size_mb.unwrap_or(DEFAULT_AB_MIN_STATE_MB);
            layout.efi_size_mb + root_a + root_b + state
        }
        InstallMode::Mutable => {
            let root = layout.root_size_mb.unwrap_or(DEFAULT_MUTABLE_MIN_ROOT_MB);
            layout.efi_size_mb + root
        }
    }
}

pub fn build_templates(layout: &ResolvedLayout) -> Vec<PartitionTemplate> {
    match layout.mode {
        InstallMode::Ab => vec![
            PartitionTemplate {
                index: 1,
                name: "efi".to_string(),
                filesystem: "vfat".to_string(),
                label: "EFI".to_string(),
                gpt_type: "U".to_string(),
                size_mb: Some(layout.efi_size_mb),
                mountpoint: "/boot".to_string(),
            },
            PartitionTemplate {
                index: 2,
                name: "root_a".to_string(),
                filesystem: "ext4".to_string(),
                label: "ROOT_A".to_string(),
                gpt_type: "L".to_string(),
                size_mb: layout.root_a_size_mb,
                mountpoint: "/slots/A".to_string(),
            },
            PartitionTemplate {
                index: 3,
                name: "root_b".to_string(),
                filesystem: "ext4".to_string(),
                label: "ROOT_B".to_string(),
                gpt_type: "L".to_string(),
                size_mb: layout.root_b_size_mb,
                mountpoint: "/slots/B".to_string(),
            },
            PartitionTemplate {
                index: 4,
                name: "state".to_string(),
                filesystem: "ext4".to_string(),
                label: "STATE".to_string(),
                gpt_type: "L".to_string(),
                size_mb: layout.state_size_mb,
                mountpoint: "/state".to_string(),
            },
        ],
        InstallMode::Mutable => vec![
            PartitionTemplate {
                index: 1,
                name: "efi".to_string(),
                filesystem: "vfat".to_string(),
                label: "EFI".to_string(),
                gpt_type: "U".to_string(),
                size_mb: Some(layout.efi_size_mb),
                mountpoint: "/boot".to_string(),
            },
            PartitionTemplate {
                index: 2,
                name: "root".to_string(),
                filesystem: "ext4".to_string(),
                label: "ROOT".to_string(),
                gpt_type: "L".to_string(),
                size_mb: layout.root_size_mb,
                mountpoint: "/".to_string(),
            },
        ],
    }
}

fn validate_positive(field: &str, value: u32) -> Result<u32> {
    if value > 0 {
        return Ok(value);
    }

    Err(RecpartError::new(
        ErrorCode::PlanGeneration,
        "policy",
        format!("{field} is greater than 0"),
        format!("{field}={value}"),
        format!(
            "Provide a positive value for --{}.",
            field.replace('_', "-")
        ),
    ))
}

fn validate_request_fields(mode: InstallMode, request: &LayoutRequest) -> Result<()> {
    match mode {
        InstallMode::Ab => {
            if request.root_size_mb.is_some() {
                return Err(RecpartError::new(
                    ErrorCode::PlanGeneration,
                    "policy",
                    "mutable-only root_size_mb is not set for ab mode",
                    "root_size_mb provided with mode=ab".to_string(),
                    "Use --root-a-size-mb/--root-b-size-mb/--state-size-mb for ab mode.",
                ));
            }
        }
        InstallMode::Mutable => {
            let invalid = [
                ("root_a_size_mb", request.root_a_size_mb),
                ("root_b_size_mb", request.root_b_size_mb),
                ("state_size_mb", request.state_size_mb),
            ]
            .into_iter()
            .filter_map(|(k, v)| v.map(|_| k))
            .collect::<Vec<_>>();

            if !invalid.is_empty() {
                return Err(RecpartError::new(
                    ErrorCode::PlanGeneration,
                    "policy",
                    "ab-only fields are not set for mutable mode",
                    format!("invalid fields for mode=mutable: {}", invalid.join(", ")),
                    "Use only --efi-size-mb and --root-size-mb for mutable mode.",
                ));
            }
        }
    }

    Ok(())
}
