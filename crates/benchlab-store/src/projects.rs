//! Project repository.

use benchlab_core::{Accent, Id, Project, now};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, params};

use crate::{Store, StoreError};

const PROJECT_COLUMNS: &str = "id, name, description, accent, created_at, updated_at, archived_at";

impl Store {
    pub async fn list_projects(&self, include_archived: bool) -> Result<Vec<Project>, StoreError> {
        self.call(move |conn| list_projects(conn, include_archived))
            .await
    }

    pub async fn get_project(&self, id: Id) -> Result<Option<Project>, StoreError> {
        self.call(move |conn| get_project(conn, id)).await
    }

    pub async fn project_name_exists(
        &self,
        name: &str,
        excluding: Option<Id>,
    ) -> Result<Option<Id>, StoreError> {
        let name = name.trim().to_owned();
        self.call(move |conn| {
            let existing = if let Some(excluding) = excluding {
                conn.query_row(
                    "SELECT id FROM projects
                     WHERE lower(name) = lower(?1) AND archived_at IS NULL AND id <> ?2",
                    params![name, excluding.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            } else {
                conn.query_row(
                    "SELECT id FROM projects
                     WHERE lower(name) = lower(?1) AND archived_at IS NULL",
                    params![name],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            };
            match existing {
                Some(value) => Id::parse_str(&value)
                    .map(Some)
                    .map_err(|error| StoreError::Invalid(format!("invalid project id: {error}"))),
                None => Ok(None),
            }
        })
        .await
    }

    pub async fn create_project(&self, mut project: Project) -> Result<Project, StoreError> {
        project.name = project.name.trim().to_owned();
        project.description = project.description.trim().to_owned();
        let errors = project.validate();
        if !errors.is_empty() {
            return Err(StoreError::Validation(errors));
        }

        let project_id = project.id;
        let name = project.name.clone();
        let created = self
            .call(move |conn| {
                ensure_project_name_available(conn, &name, None)?;
                conn.execute(
                    "INSERT INTO projects
                       (id, name, description, accent, created_at, updated_at, archived_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                    params![
                        project_id.to_string(),
                        name,
                        project.description,
                        project.accent.as_str(),
                        timestamp(project.created_at),
                        timestamp(project.updated_at),
                    ],
                )
                .map_err(project_write_error)?;
                get_project(conn, project_id)?.ok_or(StoreError::NotFound)
            })
            .await?;
        Ok(created)
    }

    pub async fn update_project(&self, mut project: Project) -> Result<Project, StoreError> {
        project.name = project.name.trim().to_owned();
        project.description = project.description.trim().to_owned();
        let errors = project.validate();
        if !errors.is_empty() {
            return Err(StoreError::Validation(errors));
        }

        let project_id = project.id;
        let name = project.name.clone();
        let updated = self
            .call(move |conn| {
                ensure_project_name_available(conn, &name, Some(project_id))?;
                let changed = conn
                    .execute(
                        "UPDATE projects
                         SET name = ?2, description = ?3, accent = ?4, updated_at = ?5
                         WHERE id = ?1",
                        params![
                            project_id.to_string(),
                            name,
                            project.description,
                            project.accent.as_str(),
                            timestamp(now()),
                        ],
                    )
                    .map_err(project_write_error)?;
                if changed == 0 {
                    return Err(StoreError::NotFound);
                }
                get_project(conn, project_id)?.ok_or(StoreError::NotFound)
            })
            .await?;
        Ok(updated)
    }

    pub async fn archive_project(&self, id: Id, archived: bool) -> Result<Project, StoreError> {
        self.call(move |conn| {
            let existing = get_project(conn, id)?.ok_or(StoreError::NotFound)?;
            if !archived {
                ensure_project_name_available(conn, &existing.name, Some(id))?;
            }
            let changed = conn
                .execute(
                    "UPDATE projects
                     SET archived_at = ?2, updated_at = ?3
                     WHERE id = ?1",
                    params![
                        id.to_string(),
                        archived.then(|| timestamp(now())),
                        timestamp(now()),
                    ],
                )
                .map_err(project_write_error)?;
            if changed == 0 {
                return Err(StoreError::NotFound);
            }
            get_project(conn, id)?.ok_or(StoreError::NotFound)
        })
        .await
    }

    pub async fn delete_project(&self, id: Id) -> Result<(), StoreError> {
        self.call(move |conn| {
            let transaction = conn.transaction()?;
            let changed = transaction.execute(
                "DELETE FROM projects WHERE id = ?1",
                params![id.to_string()],
            )?;
            if changed == 0 {
                return Err(StoreError::NotFound);
            }
            transaction.commit()?;
            Ok(())
        })
        .await
    }
}

fn list_projects(
    conn: &mut Connection,
    include_archived: bool,
) -> Result<Vec<Project>, StoreError> {
    let sql = if include_archived {
        format!("SELECT {PROJECT_COLUMNS} FROM projects ORDER BY updated_at DESC, lower(name)")
    } else {
        format!(
            "SELECT {PROJECT_COLUMNS} FROM projects
             WHERE archived_at IS NULL ORDER BY updated_at DESC, lower(name)"
        )
    };
    let mut statement = conn.prepare(&sql)?;
    let rows = statement
        .query_map([], project_row)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(Project::try_from).collect()
}

fn get_project(conn: &Connection, id: Id) -> Result<Option<Project>, StoreError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"
    ))?;
    let row = statement
        .query_row(params![id.to_string()], project_row)
        .optional()?;
    row.map(Project::try_from).transpose()
}

fn ensure_project_name_available(
    conn: &Connection,
    name: &str,
    excluding: Option<Id>,
) -> Result<(), StoreError> {
    let existing: Option<String> = if let Some(excluding) = excluding {
        conn.query_row(
            "SELECT id FROM projects
             WHERE lower(name) = lower(?1) AND archived_at IS NULL AND id <> ?2",
            params![name, excluding.to_string()],
            |row| row.get(0),
        )
        .optional()?
    } else {
        conn.query_row(
            "SELECT id FROM projects
             WHERE lower(name) = lower(?1) AND archived_at IS NULL",
            params![name],
            |row| row.get(0),
        )
        .optional()?
    };
    if existing.is_some() {
        Err(StoreError::Conflict(format!(
            "a project named “{name}” already exists"
        )))
    } else {
        Ok(())
    }
}

fn project_write_error(error: rusqlite::Error) -> StoreError {
    if let rusqlite::Error::SqliteFailure(code, _) = &error
        && code.code == ErrorCode::ConstraintViolation
    {
        return StoreError::Conflict("project name must be unique".to_owned());
    }
    StoreError::from(error)
}

struct ProjectRow {
    id: String,
    name: String,
    description: String,
    accent: String,
    created_at: String,
    updated_at: String,
    archived_at: Option<String>,
}

fn project_row(row: &Row<'_>) -> rusqlite::Result<ProjectRow> {
    Ok(ProjectRow {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        accent: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        archived_at: row.get(6)?,
    })
}

impl TryFrom<ProjectRow> for Project {
    type Error = StoreError;

    fn try_from(row: ProjectRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: parse_id(&row.id)?,
            name: row.name,
            description: row.description,
            accent: Accent::from_db(&row.accent).ok_or_else(|| {
                StoreError::Invalid(format!("unknown project accent: {}", row.accent))
            })?,
            created_at: parse_time(&row.created_at)?,
            updated_at: parse_time(&row.updated_at)?,
            archived_at: row.archived_at.as_deref().map(parse_time).transpose()?,
        })
    }
}

pub(crate) fn parse_id(value: &str) -> Result<Id, StoreError> {
    Id::parse_str(value).map_err(|error| StoreError::Invalid(format!("invalid id: {error}")))
}

pub(crate) fn parse_time(value: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|error| StoreError::Invalid(format!("invalid timestamp: {error}")))
}

pub(crate) fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Store;

    fn project(name: &str) -> Project {
        Project::new(name, "", Accent::Blue).unwrap()
    }

    #[test]
    fn projects_round_trip_and_names_are_case_insensitive() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let created = store
                .create_project(project("Inventory API"))
                .await
                .unwrap();
            assert_eq!(
                store.get_project(created.id).await.unwrap().unwrap().name,
                "Inventory API"
            );

            let duplicate = store.create_project(project("inventory api")).await;
            assert!(matches!(duplicate, Err(StoreError::Conflict(_))));

            let mut renamed = created.clone();
            renamed.name = "Checkout API".into();
            store.update_project(renamed).await.unwrap();
            let names = store.list_projects(false).await.unwrap();
            assert_eq!(names[0].name, "Checkout API");
        });
    }

    #[test]
    fn archived_projects_are_hidden_but_retained() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let created = store.create_project(project("Archive me")).await.unwrap();
            store.archive_project(created.id, true).await.unwrap();
            assert!(store.list_projects(false).await.unwrap().is_empty());
            assert_eq!(store.list_projects(true).await.unwrap().len(), 1);
            assert!(
                store
                    .project_name_exists("Archive me", None)
                    .await
                    .unwrap()
                    .is_none()
            );
        });
    }

    #[test]
    fn restoring_a_project_does_not_steal_an_active_name() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let archived = store.create_project(project("Shared name")).await.unwrap();
            store.archive_project(archived.id, true).await.unwrap();
            store.create_project(project("shared name")).await.unwrap();

            let restored = store.archive_project(archived.id, false).await;
            assert!(matches!(restored, Err(StoreError::Conflict(_))));
        });
    }

    #[test]
    fn deleting_a_project_cascades_environments() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let created = store.create_project(project("Cascade")).await.unwrap();
            let environment = benchlab_core::Environment::new(
                created.id,
                "Local",
                benchlab_core::EnvironmentKind::Local,
                "http://localhost:3000",
            )
            .unwrap();
            store.save_environment(environment.clone()).await.unwrap();

            store.delete_project(created.id).await.unwrap();
            assert!(
                store
                    .get_environment(environment.id)
                    .await
                    .unwrap()
                    .is_none()
            );
        });
    }
}
