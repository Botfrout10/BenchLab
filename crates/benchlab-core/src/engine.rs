//! Engine status is shared by the app shell and the engine adapter.
//!
//! Keeping the view-facing status type in `core` lets `benchlab-ui` display
//! engine readiness without depending on `benchlab-engine` (SPEC.md §9.2).

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineStatus {
    pub found: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub supported: bool,
    pub message: String,
}

impl EngineStatus {
    pub fn ready(&self) -> bool {
        self.found && self.supported
    }

    pub fn summary(&self) -> String {
        self.message.clone()
    }
}
