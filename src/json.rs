use crate::error::{ErrorCode, RecpartError, Result};
use serde::Serialize;

pub fn to_pretty_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|err| {
        RecpartError::new(
            ErrorCode::JsonSerialize,
            "json",
            "value serializes as pretty JSON",
            err.to_string(),
            "Inspect serialization types and schema constraints.",
        )
    })
}
