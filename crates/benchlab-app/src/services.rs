//! Application bootstrap and the concrete `BenchLabBackend`.
//!
//! This is the only crate that knows about SQLite, the OS keychain, and the
//! engine together. The UI receives the narrow trait in `benchlab_ui::backend`.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, SystemTime},
};

use crate::mock::MockTargetManager;
use async_trait::async_trait;
use benchlab_core::{APP_VERSION, EngineStatus, Environment, Id, Project, now};
use benchlab_engine::detect_k6;
use benchlab_store::{
    AppPaths, InstanceLock, LockError, SecretError, SecretPersistence, SecretStore, Store,
    StoreError, default_secret_store,
};
use benchlab_ui::backend::{
    AppView, BackendError, BackendResult, BenchLabBackend, SecretStorage, SecretUpdate,
};
use secrecy::{ExposeSecret, SecretString};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

/// Everything that must live for the whole application run.
pub struct AppInstance {
    pub services: AppServices,
    _lock: InstanceLock,
    _log_guard: WorkerGuard,
}

#[derive(Clone)]
pub struct AppServices {
    pub(crate) paths: AppPaths,
    pub(crate) store: Store,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) engine: EngineStatus,
    pub(crate) mock: Arc<MockTargetManager>,
}

impl AppServices {
    pub fn view(&self) -> AppView {
        AppView {
            app_version: APP_VERSION.to_owned(),
            engine: self.engine.clone(),
            data_dir: self.paths.data_dir().to_path_buf(),
            db_path: self.paths.db_path(),
            secret_storage: match self.secrets.persistence() {
                SecretPersistence::Keychain => SecretStorage::Keychain,
                SecretPersistence::SessionOnly => SecretStorage::SessionOnly,
            },
        }
    }

    fn secret_refs(environment: &Environment) -> HashSet<Id> {
        environment
            .variables
            .iter()
            .filter_map(|variable| variable.secret.then_some(variable.secret_ref).flatten())
            .collect()
    }

    async fn persist_environment(
        &self,
        environment: Environment,
        updates: Vec<SecretUpdate>,
    ) -> BackendResult<Environment> {
        let errors = environment.validate();
        if !errors.is_empty() {
            return Err(BackendError {
                message: "Fix the highlighted fields.".to_owned(),
                fields: errors,
            });
        }

        let old = self
            .store
            .get_environment(environment.id)
            .await
            .map_err(map_store_error)?;
        let old_refs = old.as_ref().map(Self::secret_refs).unwrap_or_default();
        let new_refs = Self::secret_refs(&environment);

        let mut previous: HashMap<Id, Option<SecretString>> = HashMap::new();
        for update in &updates {
            if !new_refs.contains(&update.secret_ref) {
                return Err(BackendError::new(
                    "Internal secret metadata mismatch; nothing was saved.",
                ));
            }
            if let std::collections::hash_map::Entry::Vacant(entry) =
                previous.entry(update.secret_ref)
            {
                let old_value = self
                    .secrets
                    .get(update.secret_ref)
                    .map_err(map_secret_error)?;
                entry.insert(old_value);
            }
            self.secrets
                .set(update.secret_ref, SecretString::from(update.value.clone()))
                .map_err(map_secret_error)?;
        }

        // Every secret variable must have a value available before committing
        // metadata that would point at an empty keychain entry.
        for secret_ref in &new_refs {
            let available = if updates
                .iter()
                .any(|update| update.secret_ref == *secret_ref)
            {
                true
            } else {
                self.secrets
                    .get(*secret_ref)
                    .map_err(map_secret_error)?
                    .is_some()
            };
            if !available {
                rollback_secrets(&*self.secrets, &previous);
                return Err(BackendError::new(
                    "A stored secret is unavailable. Re-enter secret values after restarting, then save.",
                ));
            }
        }

        let saved = match self.store.save_environment(environment).await {
            Ok(saved) => saved,
            Err(error) => {
                rollback_secrets(&*self.secrets, &previous);
                return Err(map_store_error(error));
            }
        };

        for stale in old_refs.difference(&new_refs) {
            if let Err(error) = self.secrets.delete(*stale) {
                tracing::warn!(secret_ref = %stale, %error, "could not delete stale keychain entry");
            }
        }

        Ok(saved)
    }

    async fn unique_environment_name(&self, project_id: Id, base: &str) -> BackendResult<String> {
        let environments = self
            .store
            .list_environments(project_id, true)
            .await
            .map_err(map_store_error)?;
        let names: HashSet<String> = environments
            .iter()
            .map(|environment| environment.name.to_lowercase())
            .collect();
        let mut candidate = truncate_name(&format!("{base} copy"), 60);
        let mut suffix = 2;
        while names.contains(&candidate.to_lowercase()) {
            let suffix_text = format!(" {suffix}");
            let head = truncate_name(base, 60usize.saturating_sub(suffix_text.chars().count()));
            candidate = format!("{head}{suffix_text}");
            suffix += 1;
            if suffix > 99 {
                return Err(BackendError::new("Could not find a free environment name."));
            }
        }
        Ok(candidate)
    }
}

fn rollback_secrets(secrets: &dyn SecretStore, previous: &HashMap<Id, Option<SecretString>>) {
    for (secret_ref, old_value) in previous {
        let result = match old_value {
            Some(value) => secrets.set(*secret_ref, value.clone()),
            None => secrets.delete(*secret_ref),
        };
        if let Err(error) = result {
            tracing::warn!(secret_ref = %secret_ref, %error, "could not roll back secret update");
        }
    }
}

pub fn boot() -> Result<AppInstance, String> {
    let paths = AppPaths::discover().map_err(|error| format!("{error}"))?;
    paths
        .ensure_directories()
        .map_err(|error| format!("could not create the data directory: {error}"))?;

    let log_guard = init_logging(&paths);
    tracing::info!(
        version = APP_VERSION,
        data_dir = %paths.data_dir().display(),
        "starting BenchLab"
    );
    cleanup_old_logs(&paths);

    let lock = InstanceLock::acquire(paths.lock_path()).map_err(|error| match error {
        LockError::AlreadyRunning(path) => {
            format!(
                "BenchLab is already running.\n\nData directory lock: {}",
                path.display()
            )
        }
        LockError::Io(error) => format!("Could not acquire the single-instance lock: {error}"),
    })?;

    let store = pollster::block_on(Store::open(paths.db_path()))
        .map_err(|error| format!("Could not open the workspace database: {error}"))?;
    let secrets: Arc<dyn SecretStore> = default_secret_store();
    if secrets.persistence() == SecretPersistence::SessionOnly {
        tracing::warn!("OS keychain unavailable; using session-only secret storage for this run");
    }
    let engine = detect_k6();
    tracing::info!(engine = %engine.summary(), "engine detected");

    Ok(AppInstance {
        services: AppServices {
            paths,
            store,
            secrets,
            engine,
            mock: Arc::new(MockTargetManager::default()),
        },
        _lock: lock,
        _log_guard: log_guard,
    })
}

fn init_logging(paths: &AppPaths) -> WorkerGuard {
    let file_appender = tracing_appender::rolling::daily(paths.logs_dir(), "benchlab.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("benchlab=info,benchlab_store=info,info"));
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(true)
            .with_writer(writer)
            .with_filter(filter),
    );
    if let Err(error) = subscriber.try_init() {
        eprintln!("BenchLab logging was already initialized: {error}");
    }
    guard
}

fn cleanup_old_logs(paths: &AppPaths) {
    let Ok(entries) = std::fs::read_dir(paths.logs_dir()) else {
        return;
    };
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(7 * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified < cutoff {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn map_store_error(error: StoreError) -> BackendError {
    match error {
        StoreError::Validation(fields) => BackendError {
            message: "Fix the highlighted fields.".to_owned(),
            fields,
        },
        StoreError::NotFound => {
            BackendError::new("That item no longer exists. Refresh and try again.")
        }
        StoreError::Conflict(message) => BackendError::new(message),
        StoreError::UnsupportedSchema(version) => BackendError::new(format!(
            "The database was created by a newer BenchLab (schema {version})."
        )),
        other => BackendError::new(other.to_string()),
    }
}

fn map_secret_error(error: SecretError) -> BackendError {
    BackendError::new(format!("Secret storage failed: {error}"))
}

fn truncate_name(name: &str, max_chars: usize) -> String {
    let trimmed = name.trim();
    let mut output: String = trimmed.chars().take(max_chars).collect();
    while output.len() > max_chars {
        output.pop();
    }
    output.trim().to_owned()
}

#[async_trait]
impl BenchLabBackend for AppServices {
    async fn list_projects(&self, include_archived: bool) -> BackendResult<Vec<Project>> {
        self.store
            .list_projects(include_archived)
            .await
            .map_err(map_store_error)
    }

    async fn last_project_id(&self) -> BackendResult<Option<Id>> {
        self.store.last_project_id().await.map_err(map_store_error)
    }

    async fn create_project(&self, project: Project) -> BackendResult<Project> {
        self.store
            .create_project(project)
            .await
            .map_err(map_store_error)
    }

    async fn update_project(&self, project: Project) -> BackendResult<Project> {
        self.store
            .update_project(project)
            .await
            .map_err(map_store_error)
    }

    async fn archive_project(&self, id: Id, archived: bool) -> BackendResult<Project> {
        self.store
            .archive_project(id, archived)
            .await
            .map_err(map_store_error)
    }

    async fn delete_project(&self, id: Id) -> BackendResult<()> {
        let environments = self
            .store
            .list_environments(id, true)
            .await
            .map_err(map_store_error)?;
        let refs: HashSet<Id> = environments
            .iter()
            .flat_map(AppServices::secret_refs)
            .collect();

        self.store
            .delete_project(id)
            .await
            .map_err(map_store_error)?;
        for secret_ref in refs {
            if let Err(error) = self.secrets.delete(secret_ref) {
                tracing::warn!(%secret_ref, %error, "could not delete project secret");
            }
        }
        Ok(())
    }

    async fn set_last_project(&self, id: Option<Id>) -> BackendResult<()> {
        self.store
            .set_last_project(id)
            .await
            .map_err(map_store_error)
    }

    async fn list_environments(
        &self,
        project_id: Id,
        include_archived: bool,
    ) -> BackendResult<Vec<Environment>> {
        self.store
            .list_environments(project_id, include_archived)
            .await
            .map_err(map_store_error)
    }

    async fn save_environment(
        &self,
        environment: Environment,
        secrets: Vec<SecretUpdate>,
    ) -> BackendResult<Environment> {
        self.persist_environment(environment, secrets).await
    }

    async fn duplicate_environment(&self, id: Id) -> BackendResult<Environment> {
        let source = self
            .store
            .get_environment(id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| BackendError::new("That environment no longer exists."))?;
        let name = self
            .unique_environment_name(source.project_id, &source.name)
            .await?;

        let mut duplicate = source.clone();
        duplicate.id = Id::now_v7();
        duplicate.name = name;
        duplicate.revision = 1;
        duplicate.created_at = now();
        duplicate.updated_at = now();
        duplicate.archived_at = None;

        let mut updates = Vec::new();
        for variable in &mut duplicate.variables {
            if variable.secret {
                let old_ref = variable.secret_ref.ok_or_else(|| {
                    BackendError::new("This environment has invalid secret metadata.")
                })?;
                let value = self
                    .secrets
                    .get(old_ref)
                    .map_err(map_secret_error)?
                    .ok_or_else(|| {
                        BackendError::new(
                            "A secret is unavailable, so it cannot be duplicated. Re-enter secrets first.",
                        )
                    })?;
                let new_ref = Id::now_v7();
                variable.secret_ref = Some(new_ref);
                updates.push(SecretUpdate {
                    secret_ref: new_ref,
                    value: value.expose_secret().to_owned(),
                });
            }
        }

        self.persist_environment(duplicate, updates).await
    }

    async fn archive_environment(&self, id: Id, archived: bool) -> BackendResult<Environment> {
        self.store
            .archive_environment(id, archived)
            .await
            .map_err(map_store_error)
    }

    async fn delete_environment(&self, id: Id) -> BackendResult<()> {
        let environment = self
            .store
            .get_environment(id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| BackendError::new("That environment no longer exists."))?;
        let refs = AppServices::secret_refs(&environment);
        self.store
            .delete_environment(id)
            .await
            .map_err(map_store_error)?;
        for secret_ref in refs {
            if let Err(error) = self.secrets.delete(secret_ref) {
                tracing::warn!(%secret_ref, %error, "could not delete environment secret");
            }
        }
        Ok(())
    }

    async fn list_scenarios(
        &self,
        project_id: Id,
        include_archived: bool,
    ) -> BackendResult<Vec<benchlab_core::Scenario>> {
        self.store
            .list_scenarios(project_id, include_archived)
            .await
            .map_err(map_store_error)
    }

    async fn save_scenario(
        &self,
        scenario: benchlab_core::Scenario,
    ) -> BackendResult<benchlab_core::Scenario> {
        self.store
            .save_scenario(scenario)
            .await
            .map_err(map_store_error)
    }

    async fn duplicate_scenario(&self, id: Id) -> BackendResult<benchlab_core::Scenario> {
        let source = self
            .store
            .get_scenario(id)
            .await
            .map_err(map_store_error)?
            .ok_or_else(|| BackendError::new("That scenario no longer exists."))?;
        let scenarios = self
            .store
            .list_scenarios(source.project_id, true)
            .await
            .map_err(map_store_error)?;
        let names: HashSet<String> = scenarios
            .iter()
            .map(|scenario| scenario.name.to_lowercase())
            .collect();
        let mut candidate = truncate_name(&format!("{} copy", source.name), 80);
        let mut suffix = 2;
        while names.contains(&candidate.to_lowercase()) {
            let suffix_text = format!(" {suffix}");
            let head = truncate_name(
                &source.name,
                80usize.saturating_sub(suffix_text.chars().count()),
            );
            candidate = format!("{head}{suffix_text}");
            suffix += 1;
            if suffix > 99 {
                return Err(BackendError::new("Could not find a free scenario name."));
            }
        }

        let mut duplicate = source.clone();
        duplicate.id = Id::now_v7();
        duplicate.name = candidate;
        duplicate.revision = 1;
        duplicate.created_at = now();
        duplicate.updated_at = now();
        duplicate.archived_at = None;
        for step in &mut duplicate.steps {
            step.id = Id::now_v7();
        }
        self.store
            .save_scenario(duplicate)
            .await
            .map_err(map_store_error)
    }

    async fn archive_scenario(
        &self,
        id: Id,
        archived: bool,
    ) -> BackendResult<benchlab_core::Scenario> {
        self.store
            .archive_scenario(id, archived)
            .await
            .map_err(map_store_error)
    }

    async fn delete_scenario(&self, id: Id) -> BackendResult<()> {
        self.store
            .delete_scenario(id)
            .await
            .map_err(map_store_error)
    }

    async fn test_step(
        &self,
        request: benchlab_ui::backend::TestStepRequest,
    ) -> BackendResult<benchlab_ui::backend::TestStepResponse> {
        crate::test_request::execute(self, request).await
    }

    async fn open_demo(&self) -> BackendResult<benchlab_ui::backend::DemoView> {
        crate::demo::open_demo(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use benchlab_core::{Accent, AuthConfig, EnvironmentKind, SafetyCaps, Variable};
    use benchlab_store::SessionSecretStore;
    use secrecy::ExposeSecret;

    fn services(store: Store, paths: AppPaths) -> AppServices {
        AppServices {
            paths,
            store,
            secrets: Arc::new(SessionSecretStore::default()),
            mock: Arc::new(MockTargetManager::default()),
            engine: EngineStatus {
                found: true,
                path: None,
                version: Some("2.2.0".to_owned()),
                supported: true,
                message: "k6 2.2.0 ready".to_owned(),
            },
        }
    }

    fn project_and_environment(services: &AppServices) -> (Project, Environment) {
        pollster::block_on(async {
            let project = services
                .create_project(Project::new("Gate B", "", Accent::Blue).unwrap())
                .await
                .unwrap();
            let mut environment = Environment::new(
                project.id,
                "Local",
                EnvironmentKind::Local,
                "https://api.example.com",
            )
            .unwrap();
            let secret_ref = Id::now_v7();
            environment
                .variables
                .push(Variable::secret("API_KEY", secret_ref));
            environment.auth = AuthConfig::None;
            environment.caps = SafetyCaps {
                max_vus: 100,
                max_rps: Some(50),
                max_duration_s: 60,
                require_confirmation: false,
            };
            (project, environment)
        })
    }

    #[test]
    fn secrets_are_keychain_only_copied_and_deleted_with_the_environment() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(temp.path());
        let store = pollster::block_on(Store::open(paths.db_path())).unwrap();
        let services = services(store, paths.clone());
        let (_project, environment) = project_and_environment(&services);
        let secret_ref = environment.variables[0].secret_ref.unwrap();
        let secret_value = "gate-b-super-secret-value";

        let saved = pollster::block_on(services.save_environment(
            environment,
            vec![SecretUpdate {
                secret_ref,
                value: secret_value.to_owned(),
            }],
        ))
        .unwrap();
        assert_eq!(
            services
                .secrets
                .get(secret_ref)
                .unwrap()
                .unwrap()
                .expose_secret(),
            secret_value
        );

        let duplicate = pollster::block_on(services.duplicate_environment(saved.id)).unwrap();
        let duplicate_ref = duplicate.variables[0].secret_ref.unwrap();
        assert_ne!(secret_ref, duplicate_ref);
        assert_eq!(
            services
                .secrets
                .get(duplicate_ref)
                .unwrap()
                .unwrap()
                .expose_secret(),
            secret_value
        );

        pollster::block_on(services.delete_environment(saved.id)).unwrap();
        assert!(services.secrets.get(secret_ref).unwrap().is_none());
        assert!(services.secrets.get(duplicate_ref).unwrap().is_some());

        // Read every SQLite file, including the WAL, and ensure the secret
        // value was never serialized into the database.
        drop(services);
        let mut database_bytes = Vec::new();
        for entry in std::fs::read_dir(temp.path()).unwrap().flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("benchlab.db")
            {
                // The WAL/SHM sidecars can disappear while the store's
                // dedicated thread shuts down, so a missing file is expected.
                if let Ok(bytes) = std::fs::read(entry.path()) {
                    database_bytes.extend(bytes);
                }
            }
        }
        assert!(
            !database_bytes
                .windows(secret_value.len())
                .any(|window| window == secret_value.as_bytes())
        );
    }

    #[test]
    fn saving_a_secret_without_a_stored_value_is_rejected() {
        let store = pollster::block_on(Store::open_in_memory()).unwrap();
        let services = services(store, AppPaths::at("/tmp/benchlab-test"));
        let (_project, environment) = project_and_environment(&services);
        let error =
            pollster::block_on(services.save_environment(environment, Vec::new())).unwrap_err();
        assert!(
            error.message.contains("secret is unavailable") || error.message.contains("Secret")
        );
    }
}
