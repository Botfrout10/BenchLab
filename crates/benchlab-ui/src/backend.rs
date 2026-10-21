//! The narrow service interface consumed by GPUI views.
//!
//! `benchlab-ui` does not depend on `benchlab-store` or `benchlab-engine`;
//! `benchlab-app` implements this trait with the real SQLite store and secret
//! backend (SPEC.md §9.2).

use std::{
    fmt,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use benchlab_core::{
    AssertionResult, Environment, ExtractionResult, FieldError, Id, Method, Project, Scenario,
    ScenarioVariable,
};
use gpui::SharedString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretStorage {
    Keychain,
    SessionOnly,
}

impl SecretStorage {
    pub fn label(self) -> &'static str {
        match self {
            SecretStorage::Keychain => "OS keychain",
            SecretStorage::SessionOnly => "Session only",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct BackendError {
    pub message: String,
    pub fields: Vec<FieldError>,
}

impl BackendError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fields: Vec::new(),
        }
    }

    pub fn field(message: impl Into<String>, field: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            message: message.clone(),
            fields: vec![FieldError::new(field, message)],
        }
    }

    pub fn field_message(&self, field: &str) -> Option<SharedString> {
        self.fields
            .iter()
            .find(|error| error.field == field)
            .map(|error| SharedString::from(error.message.clone()))
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

pub type BackendResult<T> = Result<T, BackendError>;

#[derive(Clone, Debug)]
pub struct TestStepRequest {
    pub scenario: Scenario,
    pub step_index: usize,
    pub environment_id: Id,
    /// Values extracted by earlier single-request tests in this editor
    /// session. They are transient and never persisted by the tester.
    pub extra_variables: Vec<ScenarioVariable>,
}

#[derive(Clone, Debug)]
pub struct TestRequestPreview {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TestStepResponse {
    pub status: u16,
    pub duration_ms: u64,
    pub size_bytes: u64,
    pub truncated: bool,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub request: TestRequestPreview,
    pub assertions: Vec<AssertionResult>,
    pub extractions: Vec<ExtractionResult>,
}

#[derive(Clone, Debug)]
pub struct DemoView {
    pub project_id: Id,
    pub scenario_id: Id,
    pub base_url: String,
}

#[derive(Clone, Debug)]
pub struct SecretUpdate {
    pub secret_ref: Id,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct AppView {
    pub app_version: String,
    pub engine: benchlab_core::EngineStatus,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub secret_storage: SecretStorage,
}

impl AppView {
    pub fn data_dir_label(&self) -> String {
        short_path(&self.data_dir)
    }

    pub fn db_label(&self) -> String {
        short_path(&self.db_path)
    }
}

fn short_path(path: &Path) -> String {
    let text = path.display().to_string();
    let count = text.chars().count();
    if count > 52 {
        let tail: String = text.chars().skip(count - 49).collect();
        format!("…{tail}")
    } else {
        text
    }
}

#[async_trait]
pub trait BenchLabBackend: Send + Sync {
    async fn list_projects(&self, include_archived: bool) -> BackendResult<Vec<Project>>;
    async fn last_project_id(&self) -> BackendResult<Option<Id>>;
    async fn create_project(&self, project: Project) -> BackendResult<Project>;
    async fn update_project(&self, project: Project) -> BackendResult<Project>;
    async fn archive_project(&self, id: Id, archived: bool) -> BackendResult<Project>;
    async fn delete_project(&self, id: Id) -> BackendResult<()>;
    async fn set_last_project(&self, id: Option<Id>) -> BackendResult<()>;

    async fn list_environments(
        &self,
        project_id: Id,
        include_archived: bool,
    ) -> BackendResult<Vec<Environment>>;
    async fn save_environment(
        &self,
        environment: Environment,
        secrets: Vec<SecretUpdate>,
    ) -> BackendResult<Environment>;
    async fn duplicate_environment(&self, id: Id) -> BackendResult<Environment>;
    async fn archive_environment(&self, id: Id, archived: bool) -> BackendResult<Environment>;
    async fn delete_environment(&self, id: Id) -> BackendResult<()>;

    async fn list_scenarios(
        &self,
        project_id: Id,
        include_archived: bool,
    ) -> BackendResult<Vec<Scenario>>;
    async fn save_scenario(&self, scenario: Scenario) -> BackendResult<Scenario>;
    async fn duplicate_scenario(&self, id: Id) -> BackendResult<Scenario>;
    async fn archive_scenario(&self, id: Id, archived: bool) -> BackendResult<Scenario>;
    async fn delete_scenario(&self, id: Id) -> BackendResult<()>;
    async fn test_step(&self, request: TestStepRequest) -> BackendResult<TestStepResponse>;
    async fn open_demo(&self) -> BackendResult<DemoView>;
}
