//! Domain types, validation, and metric definitions for BenchLab.
//!
//! The entity model mirrors SPEC.md §10. Persistence lives in
//! `benchlab-store`; UI code consumes these types through view models.

use serde::{Deserialize, Serialize};

pub mod domain;
pub mod engine;
pub mod scenario;

pub use domain::{
    Accent, AuthConfig, Environment, EnvironmentKind, FieldError, Project, SafetyCaps, Variable,
    is_header_name, now, validate_allowed_hosts, validate_auth, validate_base_url, validate_caps,
    validate_environment_name, validate_project, validate_variable_key, validate_variables,
};
pub use engine::EngineStatus;
pub use scenario::{
    Assertion, AssertionResult, Body, Extraction, ExtractionResult, KeyValue, Method,
    ResponseFacts, Scenario, ScenarioVariable, Step, ThinkTime, evaluate_assertions,
    evaluate_extractions, interpolate, parse_scalar_input, template_variables, validate_json_path,
    validate_scenario_name,
};

pub const APP_NAME: &str = "BenchLab";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Stable identifier used by every persisted entity.
pub type Id = uuid::Uuid;

/// Information shown in the About screen and recorded in run snapshots.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

impl AppInfo {
    pub fn current() -> Self {
        Self {
            name: APP_NAME.to_string(),
            version: APP_VERSION.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_has_a_version() {
        let info = AppInfo::current();
        assert_eq!(info.name, "BenchLab");
        assert!(!info.version.is_empty());
    }
}
