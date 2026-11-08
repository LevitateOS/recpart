use crate::error::{ErrorCode, RecpartError, Result};
use crate::types::{Filesystem, InstallMode, LayoutRequest, PartitionTemplate, ResolvedLayout, Role};

// The installed disk layout: what every tool that formats, mounts, boots or
// carries a partition reads (recpart's templates and manual layouts, recab,
// recsetup, recfstab, the install tests).

/// Where the ESP is mounted in the installed system (recab writes the boot
/// entries and kernels there).
pub const ESP_MOUNT: &str = "/boot";
/// Where the persistent state partition is mounted (recab keeps its state there).
pub const STATE_MOUNT: &str = "/var";
/// The installed system's root.
pub const ROOT_MOUNT: &str = "/";
/// Where the install mounts the new system by default.
pub const DEFAULT_MOUNT_ROOT: &str = "/mnt";

/// GPT partition type GUIDs.
pub const EFI_TYPE: &str = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B";
pub const LINUX_TYPE: &str = "0FC63DAF-8483-4772-8E79-3D69D8477DE4";
pub const SWAP_TYPE: &str = "0657FD6D-A4AB-43C4-84E5-0933C84B4F4F";
pub const BASIC_DATA_TYPE: &str = "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7";

/// One MiB, the unit of every `_mb` size.
pub const MIB: u64 = 1024 * 1024;

impl Role {
    /// The filesystem recpart makes for the role.
    pub fn filesystem(self) -> Filesystem {
        match self {
            Role::Efi => Filesystem::Vfat,
            Role::Swap => Filesystem::Swap,
            Role::Root | Role::RootA | Role::RootB | Role::State | Role::Home | Role::Other => {
                Filesystem::Ext4
            }
        }
    }

    /// The filesystem label recpart gives the role (`None`: the layout's own).
    pub fn label(self) -> Option<&'static str> {
        Some(match self {
            Role::Efi => "EFI",
            Role::Root => "ROOT",
            Role::RootA => "ROOT_A",
            Role::RootB => "ROOT_B",
            Role::State => "STATE",
            Role::Home => "HOME",
            Role::Swap => "SWAP",
            Role::Other => return None,
        })
    }

    /// The role's mount point in the installed system (`None`: not mounted,
    /// or for `other` the layout's own).
    pub fn mountpoint(self) -> Option<&'static str> {
        match self {
            Role::Efi => Some(ESP_MOUNT),
            Role::Root | Role::RootA => Some(ROOT_MOUNT),
            Role::RootB | Role::Swap | Role::Other => None,
            Role::State => Some(STATE_MOUNT),
            Role::Home => Some("/home"),
        }
    }

    /// The GPT type recpart gives a partition it creates for the role with
    /// the filesystem `fs`: the ESP's, else the filesystem's.
    pub fn gpt_type(role: Option<Role>, fs: Filesystem) -> &'static str {
        match (role, fs) {
            (Some(Role::Efi), _) => EFI_TYPE,
            (_, Filesystem::Swap) => SWAP_TYPE,
            (_, Filesystem::Vfat) => BASIC_DATA_TYPE,
            (_, Filesystem::Ext4) => LINUX_TYPE,
        }
    }

    /// The A/B slot role (`A` or `B`).
    pub fn slot(slot: char) -> Option<Role> {
        match slot.to_ascii_uppercase() {
            'A' => Some(Role::RootA),
            'B' => Some(Role::RootB),
            _ => None,
        }
    }
}

/// The partitions a mode's template creates, in order.
pub fn template_roles(mode: InstallMode) -> &'static [Role] {
    match mode {
        InstallMode::Ab => &[Role::Efi, Role::RootA, Role::RootB, Role::State],
        InstallMode::Mutable => &[Role::Efi, Role::Root],
    }
}

/// The smallest disk a mode's template fits with its default sizes, in MiB:
/// the partitions, the policy minimum of the one taking the rest, and the
/// table's own room (the first MiB and the backup GPT).
pub fn min_disk_mb(mode: InstallMode) -> u64 {
    let layout = policy_defaults(mode);
    let templates = build_templates(&layout);
    let parts: u64 = templates
        .iter()
        .map(|p| u64::from(p.size_mb.unwrap_or_else(|| remaining_min_mb(mode))))
        .sum();
    parts + 2
}

/// Mount point of a partition that is not mounted during installation (the
/// inactive A/B slot), as in fstab.
pub const NOT_MOUNTED: &str = "none";
pub const DEFAULT_EFI_SIZE_MB: u32 = 1024;
pub const DEFAULT_AB_ROOT_SIZE_MB: u32 = 20 * 1024;
pub const DEFAULT_AB_MIN_STATE_MB: u32 = 4 * 1024;
pub const DEFAULT_MUTABLE_MIN_ROOT_MB: u32 = 8 * 1024;
/// The smallest A/B root slot a manual layout may give.
pub const MIN_AB_ROOT_MB: u32 = 8 * 1024;
/// Room one slot's kernel and initramfs take on the EFI partition.
pub const SLOT_BOOT_FILES_MB: u64 = 128;
/// Room systemd-boot and its loader entries take on the EFI partition.
pub const BOOTLOADER_MB: u64 = 8;
/// The smallest EFI partition recpart formats: FAT32 on 4 KiB-sector disks needs 260 MiB.
pub const MIN_NEW_EFI_MB: u64 = 260;
/// The smallest swap partition or file.
pub const MIN_SWAP_MB: u64 = 64;

/// Free space the EFI partition needs for the mode's kernels and the boot loader.
pub fn efi_boot_files_mb(mode: InstallMode) -> u64 {
    let slots = match mode {
        InstallMode::Ab => 2,
        InstallMode::Mutable => 1,
    };
    slots * SLOT_BOOT_FILES_MB + BOOTLOADER_MB
}

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

/// The smallest size the partition that takes the remaining space may get:
/// the same minimum an explicit size must meet.
pub fn remaining_min_mb(mode: InstallMode) -> u32 {
    match mode {
        InstallMode::Ab => DEFAULT_AB_MIN_STATE_MB,
        InstallMode::Mutable => DEFAULT_MUTABLE_MIN_ROOT_MB,
    }
}

pub fn build_templates(layout: &ResolvedLayout) -> Vec<PartitionTemplate> {
    template_roles(layout.mode)
        .iter()
        .zip(1u8..)
        .map(|(&role, index)| PartitionTemplate {
            index,
            name: role.as_str().to_string(),
            filesystem: role.filesystem(),
            label: role.label().expect("template roles have labels").to_string(),
            gpt_type: Role::gpt_type(Some(role), role.filesystem()).to_string(),
            size_mb: match role {
                Role::Efi => Some(layout.efi_size_mb),
                Role::Root => layout.root_size_mb,
                Role::RootA => layout.root_a_size_mb,
                Role::RootB => layout.root_b_size_mb,
                Role::State => layout.state_size_mb,
                Role::Home | Role::Swap | Role::Other => None,
            },
            mountpoint: role.mountpoint().unwrap_or(NOT_MOUNTED).to_string(),
        })
        .collect()
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
