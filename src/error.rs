use crate::types::{ErrorPayload, ERROR_SCHEMA_VERSION};
use std::fmt;

/// recpart's error table. Each variant's number is its stable code: it prints
/// as `E0NN`, is the JSON `code`, and is the exit status. Numbers are assigned
/// explicitly and never reused (8, 9 and 10 belonged to removed variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ErrorCode {
    InvalidTargetDisk = 1,
    MissingTool = 2,
    PlanGeneration = 3,
    MissingConfirmation = 4,
    PartitionApplyFailed = 5,
    FormatFailed = 6,
    MountFailed = 7,
    NotRoot = 11,
    Internal = 12,
    InvalidMountRoot = 13,
    /// The manual layout is malformed or contradicts the disk: an unknown or
    /// twice-named partition, a role on the wrong filesystem, an MBR edit.
    InvalidLayout = 14,
    /// A created or grown partition does not fit in the free space.
    LayoutDoesNotFit = 15,
    /// The roles do not make the mode's layout (`ab`: EFI, two equal root
    /// slots and state; `mutable`: EFI and root).
    ModeRequirements = 16,
    /// Two partitions (or roles) would mount at the same place.
    DuplicateMountpoint = 17,
    /// A partition is smaller than its role's minimum.
    PartitionTooSmall = 18,
    /// The EFI partition is not FAT, not an EFI system partition, or has no
    /// room for the slots' kernels.
    EfiUnsuitable = 19,
    /// recpart cannot resize this partition safely (its filesystem, or it is mounted).
    ResizeUnsupported = 20,
    /// A shrink below the space in use, or with the used space unreadable.
    ShrinkBelowUsed = 21,
    /// A partition the install formats or uses does not start on a 4 KiB
    /// (physical sector) boundary.
    Misaligned = 22,
    /// The disk is not the one the layout was made for: its table id changed,
    /// or it no longer gives the plan `apply --expect` names.
    DiskChanged = 23,
    /// A recorded inventory (`--inventory`) is unreadable, or was given to a real apply.
    InvalidInventory = 24,
    ResizeFailed = 25,
    SwapFailed = 26,
    /// The confirmed plan (`apply --expect`) is unreadable or not a recpart plan document.
    InvalidExpectedPlan = 27,
}

impl ErrorCode {
    pub const ALL: &'static [ErrorCode] = &[
        ErrorCode::InvalidTargetDisk,
        ErrorCode::MissingTool,
        ErrorCode::PlanGeneration,
        ErrorCode::MissingConfirmation,
        ErrorCode::PartitionApplyFailed,
        ErrorCode::FormatFailed,
        ErrorCode::MountFailed,
        ErrorCode::NotRoot,
        ErrorCode::Internal,
        ErrorCode::InvalidMountRoot,
        ErrorCode::InvalidLayout,
        ErrorCode::LayoutDoesNotFit,
        ErrorCode::ModeRequirements,
        ErrorCode::DuplicateMountpoint,
        ErrorCode::PartitionTooSmall,
        ErrorCode::EfiUnsuitable,
        ErrorCode::ResizeUnsupported,
        ErrorCode::ShrinkBelowUsed,
        ErrorCode::Misaligned,
        ErrorCode::DiskChanged,
        ErrorCode::InvalidInventory,
        ErrorCode::ResizeFailed,
        ErrorCode::SwapFailed,
        ErrorCode::InvalidExpectedPlan,
    ];

    pub fn exit_status(self) -> u8 {
        self as u8
    }

    /// The variant's name in SCREAMING_SNAKE_CASE (`DISK_CHANGED`), the name
    /// front ends know the code by.
    pub fn name(self) -> String {
        let mut name = String::new();
        for (i, c) in format!("{self:?}").chars().enumerate() {
            if c.is_ascii_uppercase() && i > 0 {
                name.push('_');
            }
            name.push(c.to_ascii_uppercase());
        }
        name
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:03}", *self as u8)
    }
}

#[derive(Debug, Clone)]
pub struct RecpartError {
    pub code: ErrorCode,
    pub component: String,
    pub expectation: String,
    pub observed: String,
    pub remediation: String,
}

impl RecpartError {
    pub fn new(
        code: ErrorCode,
        component: impl Into<String>,
        expectation: impl Into<String>,
        observed: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Self {
        Self {
            code,
            component: component.into(),
            expectation: expectation.into(),
            observed: observed.into(),
            remediation: remediation.into(),
        }
    }

    pub fn payload(&self) -> ErrorPayload {
        ErrorPayload {
            schema_version: ERROR_SCHEMA_VERSION,
            code: self.code.to_string(),
            component: self.component.clone(),
            expectation: self.expectation.clone(),
            observed: self.observed.clone(),
            remediation: self.remediation.clone(),
        }
    }
}

impl fmt::Display for RecpartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: [{}] expected {}, observed {}. remediation: {}",
            self.code,
            self.component,
            self.expectation,
            self.observed,
            self.remediation
        )
    }
}

impl std::error::Error for RecpartError {}

pub type Result<T> = std::result::Result<T, RecpartError>;
