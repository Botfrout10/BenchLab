//! Environment repository, including safety caps and secret metadata.

use benchlab_core::{AuthConfig, Environment, EnvironmentKind, Id, SafetyCaps, Variable, now};
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, params};

use crate::{
    Store, StoreError,
    projects::{parse_id, parse_time, timestamp},
};

const ENVIRONMENT_COLUMNS: &str = "id, project_id, name, kind, base_url, variables_json,
    auth_json, tls_verify, allow_external_hosts, allowed_hosts_json, caps_json,
    revision, created_at, updated_at, archived_at";

impl Store {
    pub async fn list_environments(
        &self,
        project_id: Id,
        include_archived: bool,
    ) -> Result<Vec<Environment>, StoreError> {
        self.call(move |conn| list_environments(conn, project_id, include_archived))
            .await
    }

    pub async fn get_environment(&self, id: Id) -> Result<Option<Environment>, StoreError> {
        self.call(move |conn| get_environment(conn, id)).await
    }

    /// Insert a new environment or update an existing one. Updates increment
    /// the environment revision. Secret metadata rows are reconciled from the
    /// variables; secret values themselves are never passed here.
    pub async fn save_environment(
        &self,
        mut environment: Environment,
    ) -> Result<Environment, StoreError> {
        environment.normalize();
        let errors = environment.validate();
        if !errors.is_empty() {
            return Err(StoreError::Validation(errors));
        }

        let project_id = environment.project_id;
        let environment_id = environment.id;
        let name = environment.name.clone();
        let kind = environment.kind.as_str();
        let base_url = environment.base_url.clone();
        let variables_json = serde_json::to_string(&environment.variables)?;
        let auth_json = serde_json::to_string(&environment.auth)?;
        let allowed_hosts_json = serde_json::to_string(&environment.allowed_hosts)?;
        let caps_json = serde_json::to_string(&environment.caps)?;
        let tls_verify = environment.tls_verify;
        let allow_external_hosts = environment.allow_external_hosts;
        let secret_refs: Vec<(String, Id)> = environment
            .variables
            .iter()
            .filter_map(|variable| {
                variable
                    .secret
                    .then(|| variable.secret_ref.map(|id| (variable.key.clone(), id)))
                    .flatten()
            })
            .collect();

        self.call(move |conn| {
            let transaction = conn.transaction()?;

            let project_exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
                params![project_id.to_string()],
                |row| row.get(0),
            )?;
            if !project_exists {
                return Err(StoreError::NotFound);
            }

            ensure_environment_name_available(
                &transaction,
                project_id,
                &name,
                Some(environment_id),
            )?;

            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM environments WHERE id = ?1)",
                params![environment_id.to_string()],
                |row| row.get(0),
            )?;

            if exists {
                transaction
                    .execute(
                        "UPDATE environments SET
                       name = ?2, kind = ?3, base_url = ?4, variables_json = ?5,
                       auth_json = ?6, tls_verify = ?7, allow_external_hosts = ?8,
                       allowed_hosts_json = ?9, caps_json = ?10,
                       revision = revision + 1, updated_at = ?11
                     WHERE id = ?1",
                        params![
                            environment_id.to_string(),
                            name,
                            kind,
                            base_url,
                            variables_json,
                            auth_json,
                            tls_verify as i64,
                            allow_external_hosts as i64,
                            allowed_hosts_json,
                            caps_json,
                            timestamp(now()),
                        ],
                    )
                    .map_err(environment_write_error)?;
            } else {
                let created = timestamp(now());
                transaction
                    .execute(
                        "INSERT INTO environments
                       (id, project_id, name, kind, base_url, variables_json, auth_json,
                        tls_verify, allow_external_hosts, allowed_hosts_json, caps_json,
                        revision, created_at, updated_at, archived_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?12, NULL)",
                        params![
                            environment_id.to_string(),
                            project_id.to_string(),
                            name,
                            kind,
                            base_url,
                            variables_json,
                            auth_json,
                            tls_verify as i64,
                            allow_external_hosts as i64,
                            allowed_hosts_json,
                            caps_json,
                            created,
                        ],
                    )
                    .map_err(environment_write_error)?;
            }

            transaction.execute(
                "DELETE FROM secret_refs WHERE environment_id = ?1",
                params![environment_id.to_string()],
            )?;
            for (key, secret_ref) in secret_refs {
                transaction.execute(
                    "INSERT INTO secret_refs (id, environment_id, key, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        secret_ref.to_string(),
                        environment_id.to_string(),
                        key,
                        timestamp(now()),
                    ],
                )?;
            }

            transaction.commit()?;
            get_environment(conn, environment_id)?.ok_or(StoreError::NotFound)
        })
        .await
    }

    pub async fn archive_environment(
        &self,
        id: Id,
        archived: bool,
    ) -> Result<Environment, StoreError> {
        self.call(move |conn| {
            let changed = conn.execute(
                "UPDATE environments SET archived_at = ?2, updated_at = ?3 WHERE id = ?1",
                params![
                    id.to_string(),
                    archived.then(|| timestamp(now())),
                    timestamp(now()),
                ],
            )?;
            if changed == 0 {
                return Err(StoreError::NotFound);
            }
            get_environment(conn, id)?.ok_or(StoreError::NotFound)
        })
        .await
    }

    pub async fn delete_environment(&self, id: Id) -> Result<(), StoreError> {
        self.call(move |conn| {
            let changed = conn.execute(
                "DELETE FROM environments WHERE id = ?1",
                params![id.to_string()],
            )?;
            if changed == 0 {
                return Err(StoreError::NotFound);
            }
            Ok(())
        })
        .await
    }
}

fn list_environments(
    conn: &mut Connection,
    project_id: Id,
    include_archived: bool,
) -> Result<Vec<Environment>, StoreError> {
    let sql = if include_archived {
        format!(
            "SELECT {ENVIRONMENT_COLUMNS} FROM environments
             WHERE project_id = ?1 ORDER BY lower(name)"
        )
    } else {
        format!(
            "SELECT {ENVIRONMENT_COLUMNS} FROM environments
             WHERE project_id = ?1 AND archived_at IS NULL ORDER BY lower(name)"
        )
    };
    let mut statement = conn.prepare(&sql)?;
    let rows = statement
        .query_map(params![project_id.to_string()], environment_row)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(Environment::try_from).collect()
}

fn get_environment(conn: &Connection, id: Id) -> Result<Option<Environment>, StoreError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {ENVIRONMENT_COLUMNS} FROM environments WHERE id = ?1"
    ))?;
    let row = statement
        .query_row(params![id.to_string()], environment_row)
        .optional()?;
    row.map(Environment::try_from).transpose()
}

fn ensure_environment_name_available(
    conn: &Connection,
    project_id: Id,
    name: &str,
    excluding: Option<Id>,
) -> Result<(), StoreError> {
    let existing: Option<String> = if let Some(excluding) = excluding {
        conn.query_row(
            "SELECT id FROM environments
             WHERE project_id = ?1 AND lower(name) = lower(?2) AND id <> ?3",
            params![project_id.to_string(), name, excluding.to_string()],
            |row| row.get(0),
        )
        .optional()?
    } else {
        conn.query_row(
            "SELECT id FROM environments
             WHERE project_id = ?1 AND lower(name) = lower(?2)",
            params![project_id.to_string(), name],
            |row| row.get(0),
        )
        .optional()?
    };
    if existing.is_some() {
        Err(StoreError::Conflict(format!(
            "an environment named “{name}” already exists in this project"
        )))
    } else {
        Ok(())
    }
}

fn environment_write_error(error: rusqlite::Error) -> StoreError {
    if let rusqlite::Error::SqliteFailure(code, _) = &error
        && code.code == ErrorCode::ConstraintViolation
    {
        return StoreError::Conflict(
            "environment names must be unique inside a project".to_owned(),
        );
    }
    StoreError::from(error)
}

struct EnvironmentRow {
    id: String,
    project_id: String,
    name: String,
    kind: String,
    base_url: String,
    variables_json: String,
    auth_json: String,
    tls_verify: i64,
    allow_external_hosts: i64,
    allowed_hosts_json: String,
    caps_json: String,
    revision: i64,
    created_at: String,
    updated_at: String,
    archived_at: Option<String>,
}

fn environment_row(row: &Row<'_>) -> rusqlite::Result<EnvironmentRow> {
    Ok(EnvironmentRow {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        base_url: row.get(4)?,
        variables_json: row.get(5)?,
        auth_json: row.get(6)?,
        tls_verify: row.get(7)?,
        allow_external_hosts: row.get(8)?,
        allowed_hosts_json: row.get(9)?,
        caps_json: row.get(10)?,
        revision: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        archived_at: row.get(14)?,
    })
}

impl TryFrom<EnvironmentRow> for Environment {
    type Error = StoreError;

    fn try_from(row: EnvironmentRow) -> Result<Self, Self::Error> {
        let variables: Vec<Variable> = serde_json::from_str(&row.variables_json)?;
        let auth: AuthConfig = serde_json::from_str(&row.auth_json)?;
        let allowed_hosts: Vec<String> = serde_json::from_str(&row.allowed_hosts_json)?;
        let caps: SafetyCaps = serde_json::from_str(&row.caps_json)?;
        Ok(Self {
            id: parse_id(&row.id)?,
            project_id: parse_id(&row.project_id)?,
            name: row.name,
            kind: EnvironmentKind::from_db(&row.kind).ok_or_else(|| {
                StoreError::Invalid(format!("unknown environment kind: {}", row.kind))
            })?,
            base_url: row.base_url,
            variables,
            auth,
            tls_verify: row.tls_verify != 0,
            allow_external_hosts: row.allow_external_hosts != 0,
            allowed_hosts,
            caps,
            revision: u32::try_from(row.revision)
                .map_err(|_| StoreError::Invalid("invalid environment revision".to_owned()))?,
            created_at: parse_time(&row.created_at)?,
            updated_at: parse_time(&row.updated_at)?,
            archived_at: row.archived_at.as_deref().map(parse_time).transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use benchlab_core::{Accent, Project};

    use super::*;
    use crate::Store;

    fn project() -> Project {
        Project::new("Inventory API", "", Accent::Blue).unwrap()
    }

    #[test]
    fn environments_persist_caps_and_increment_revision() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let project = store.create_project(project()).await.unwrap();
            let mut environment = Environment::new(
                project.id,
                "Local",
                EnvironmentKind::Local,
                "http://localhost:3000",
            )
            .unwrap();
            environment.caps.max_vus = 42;
            let first = store.save_environment(environment.clone()).await.unwrap();
            assert_eq!(first.revision, 1);
            assert_eq!(first.caps.max_vus, 42);

            let second = store.save_environment(environment).await.unwrap();
            assert_eq!(second.revision, 2);
            assert_eq!(second.id, first.id);
            assert_eq!(
                store
                    .list_environments(project.id, false)
                    .await
                    .unwrap()
                    .len(),
                1
            );
        });
    }

    #[test]
    fn environments_require_unique_names_in_a_project() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let project = store.create_project(project()).await.unwrap();
            let first = Environment::new(
                project.id,
                "Staging",
                EnvironmentKind::Staging,
                "https://staging.example.com",
            )
            .unwrap();
            store.save_environment(first).await.unwrap();

            let second = Environment::new(
                project.id,
                "staging",
                EnvironmentKind::Staging,
                "https://staging.example.com",
            )
            .unwrap();
            assert!(matches!(
                store.save_environment(second).await,
                Err(StoreError::Conflict(_))
            ));
        });
    }

    #[test]
    fn invalid_urls_and_caps_are_rejected_before_write() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let project = store.create_project(project()).await.unwrap();
            let mut environment = Environment::new(
                project.id,
                "Local",
                EnvironmentKind::Local,
                "http://localhost:3000",
            )
            .unwrap();
            environment.base_url = "localhost:3000".into();
            environment.caps.max_vus = 0;
            let result = store.save_environment(environment).await;
            assert!(matches!(result, Err(StoreError::Validation(_))));
        });
    }

    #[test]
    fn archived_environments_are_hidden_by_default() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let project = store.create_project(project()).await.unwrap();
            let environment = Environment::new(
                project.id,
                "Local",
                EnvironmentKind::Local,
                "http://localhost:3000",
            )
            .unwrap();
            let saved = store.save_environment(environment).await.unwrap();
            store.archive_environment(saved.id, true).await.unwrap();
            assert!(
                store
                    .list_environments(project.id, false)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                store
                    .list_environments(project.id, true)
                    .await
                    .unwrap()
                    .len(),
                1
            );
        });
    }
}
