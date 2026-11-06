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
}

impl ErrorCode {
    pub fn exit_status(self) -> u8 {
        self as u8
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
