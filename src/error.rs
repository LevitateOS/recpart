use crate::types::{ErrorPayload, ERROR_SCHEMA_VERSION};
use distro_spec::impl_error_code_display;
use distro_spec::shared::error::ToolErrorCode;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidTargetDisk = 1,
    MissingTool = 2,
    PlanGeneration = 3,
    MissingConfirmation = 4,
    PartitionApplyFailed = 5,
    FormatFailed = 6,
    MountFailed = 7,
    HandoffFailed = 8,
    JsonSerialize = 9,
    NotImplemented = 10,
    NotRoot = 11,
    Internal = 12,
}

impl ToolErrorCode for ErrorCode {
    fn code(&self) -> &'static str {
        match self {
            ErrorCode::InvalidTargetDisk => "E001",
            ErrorCode::MissingTool => "E002",
            ErrorCode::PlanGeneration => "E003",
            ErrorCode::MissingConfirmation => "E004",
            ErrorCode::PartitionApplyFailed => "E005",
            ErrorCode::FormatFailed => "E006",
            ErrorCode::MountFailed => "E007",
            ErrorCode::HandoffFailed => "E008",
            ErrorCode::JsonSerialize => "E009",
            ErrorCode::NotImplemented => "E010",
            ErrorCode::NotRoot => "E011",
            ErrorCode::Internal => "E012",
        }
    }

    fn exit_code(&self) -> u8 {
        *self as u8
    }
}

impl_error_code_display!(ErrorCode);

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
            code: self.code.code().to_string(),
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
            self.code.code(),
            self.component,
            self.expectation,
            self.observed,
            self.remediation
        )
    }
}

impl std::error::Error for RecpartError {}

pub type Result<T> = std::result::Result<T, RecpartError>;
