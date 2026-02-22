pub mod cli;
pub mod error;
pub mod exec;
pub mod handoff;
pub mod json;
pub mod plan;
pub mod policy;
pub mod preflight;
pub mod tui;
pub mod types;

pub use error::{ErrorCode, RecpartError, Result};
pub use exec::{execute_plan, execute_plan_with_reporter};
pub use plan::{build_plan, build_plan_with_layout_request};
pub use types::{
    ApplyOptions, ApplyResult, DiskTarget, HandoffPayload, InstallMode, LayoutRequest,
    PartitionPlan, ResolvedLayout,
};
