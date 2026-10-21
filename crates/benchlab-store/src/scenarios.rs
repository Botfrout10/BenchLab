//! Scenario repository.
//!
//! Scenario definitions are stored as JSON so the full step tree can evolve
//! without a wide relational schema. Static validation is intentionally not
//! enforced here: FR-SCN-14 allows an invalid scenario to be saved as a draft.
//! The name is the one structural requirement because it identifies the row.

use benchlab_core::{Id, Scenario, ScenarioVariable, Step, now, validate_scenario_name};
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::{
    Store, StoreError,
    projects::{parse_id, parse_time, timestamp},
};

const SCENARIO_COLUMNS: &str = "id, project_id, name, description, revision,
    definition_json, created_at, updated_at, archived_at";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ScenarioDefinition {
    #[serde(default)]
    variables: Vec<ScenarioVariable>,
    #[serde(default)]
    steps: Vec<Step>,
}

impl Store {
    pub async fn list_scenarios(
        &self,
        project_id: Id,
        include_archived: bool,
    ) -> Result<Vec<Scenario>, StoreError> {
        self.call(move |conn| list_scenarios(conn, project_id, include_archived))
            .await
    }

    pub async fn get_scenario(&self, id: Id) -> Result<Option<Scenario>, StoreError> {
        self.call(move |conn| get_scenario(conn, id)).await
    }

    /// Insert a new scenario or update an existing one. Every update increments
    /// the revision, including saves of invalid drafts.
    pub async fn save_scenario(&self, mut scenario: Scenario) -> Result<Scenario, StoreError> {
        scenario.normalize();
        let name_errors = validate_scenario_name(&scenario.name);
        if !name_errors.is_empty() {
            return Err(StoreError::Validation(name_errors));
        }

        let project_id = scenario.project_id;
        let scenario_id = scenario.id;
        let name = scenario.name.clone();
        let description = scenario.description.clone();
        let definition_json = serde_json::to_string(&ScenarioDefinition {
            variables: scenario.variables.clone(),
            steps: scenario.steps.clone(),
        })?;

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
            ensure_scenario_name_available(&transaction, project_id, &name, Some(scenario_id))?;

            let existing_project: Option<String> = transaction
                .query_row(
                    "SELECT project_id FROM scenarios WHERE id = ?1",
                    params![scenario_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;

            if let Some(existing_project) = existing_project {
                if existing_project != project_id.to_string() {
                    return Err(StoreError::Conflict(
                        "a scenario cannot move between projects".to_owned(),
                    ));
                }
                transaction
                    .execute(
                        "UPDATE scenarios SET
                           name = ?2, description = ?3, definition_json = ?4,
                           revision = revision + 1, updated_at = ?5
                         WHERE id = ?1",
                        params![
                            scenario_id.to_string(),
                            name,
                            description,
                            definition_json,
                            timestamp(now()),
                        ],
                    )
                    .map_err(scenario_write_error)?;
            } else {
                let created = timestamp(now());
                transaction
                    .execute(
                        "INSERT INTO scenarios
                           (id, project_id, name, description, revision, definition_json,
                            created_at, updated_at, archived_at)
                         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?6, NULL)",
                        params![
                            scenario_id.to_string(),
                            project_id.to_string(),
                            name,
                            description,
                            definition_json,
                            created,
                        ],
                    )
                    .map_err(scenario_write_error)?;
            }

            transaction.commit()?;
            get_scenario(conn, scenario_id)?.ok_or(StoreError::NotFound)
        })
        .await
    }

    pub async fn archive_scenario(&self, id: Id, archived: bool) -> Result<Scenario, StoreError> {
        self.call(move |conn| {
            let changed = conn.execute(
                "UPDATE scenarios SET archived_at = ?2, updated_at = ?3 WHERE id = ?1",
                params![
                    id.to_string(),
                    archived.then(|| timestamp(now())),
                    timestamp(now()),
                ],
            )?;
            if changed == 0 {
                return Err(StoreError::NotFound);
            }
            get_scenario(conn, id)?.ok_or(StoreError::NotFound)
        })
        .await
    }

    pub async fn delete_scenario(&self, id: Id) -> Result<(), StoreError> {
        self.call(move |conn| {
            let changed = conn.execute(
                "DELETE FROM scenarios WHERE id = ?1",
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

fn list_scenarios(
    conn: &mut Connection,
    project_id: Id,
    include_archived: bool,
) -> Result<Vec<Scenario>, StoreError> {
    let sql = if include_archived {
        format!(
            "SELECT {SCENARIO_COLUMNS} FROM scenarios
             WHERE project_id = ?1 ORDER BY lower(name)"
        )
    } else {
        format!(
            "SELECT {SCENARIO_COLUMNS} FROM scenarios
             WHERE project_id = ?1 AND archived_at IS NULL ORDER BY lower(name)"
        )
    };
    let mut statement = conn.prepare(&sql)?;
    let rows = statement
        .query_map(params![project_id.to_string()], scenario_row)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(Scenario::try_from).collect()
}

fn get_scenario(conn: &Connection, id: Id) -> Result<Option<Scenario>, StoreError> {
    let mut statement = conn.prepare(&format!(
        "SELECT {SCENARIO_COLUMNS} FROM scenarios WHERE id = ?1"
    ))?;
    let row = statement
        .query_row(params![id.to_string()], scenario_row)
        .optional()?;
    row.map(Scenario::try_from).transpose()
}

fn ensure_scenario_name_available(
    conn: &Connection,
    project_id: Id,
    name: &str,
    excluding: Option<Id>,
) -> Result<(), StoreError> {
    let existing: Option<String> = if let Some(excluding) = excluding {
        conn.query_row(
            "SELECT id FROM scenarios
             WHERE project_id = ?1 AND lower(name) = lower(?2) AND id <> ?3",
            params![project_id.to_string(), name, excluding.to_string()],
            |row| row.get(0),
        )
        .optional()?
    } else {
        conn.query_row(
            "SELECT id FROM scenarios
             WHERE project_id = ?1 AND lower(name) = lower(?2)",
            params![project_id.to_string(), name],
            |row| row.get(0),
        )
        .optional()?
    };
    if existing.is_some() {
        Err(StoreError::Conflict(format!(
            "a scenario named “{name}” already exists in this project"
        )))
    } else {
        Ok(())
    }
}

fn scenario_write_error(error: rusqlite::Error) -> StoreError {
    if let rusqlite::Error::SqliteFailure(code, _) = &error
        && code.code == ErrorCode::ConstraintViolation
    {
        return StoreError::Conflict("scenario names must be unique inside a project".to_owned());
    }
    StoreError::from(error)
}

struct ScenarioRow {
    id: String,
    project_id: String,
    name: String,
    description: String,
    revision: i64,
    definition_json: String,
    created_at: String,
    updated_at: String,
    archived_at: Option<String>,
}

fn scenario_row(row: &Row<'_>) -> rusqlite::Result<ScenarioRow> {
    Ok(ScenarioRow {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        revision: row.get(4)?,
        definition_json: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        archived_at: row.get(8)?,
    })
}

impl TryFrom<ScenarioRow> for Scenario {
    type Error = StoreError;

    fn try_from(row: ScenarioRow) -> Result<Self, Self::Error> {
        let definition: ScenarioDefinition = serde_json::from_str(&row.definition_json)?;
        Ok(Scenario {
            id: parse_id(&row.id)?,
            project_id: parse_id(&row.project_id)?,
            name: row.name,
            description: row.description,
            revision: u32::try_from(row.revision)
                .map_err(|_| StoreError::Invalid("scenario revision is out of range".to_owned()))?,
            variables: definition.variables,
            steps: definition.steps,
            created_at: parse_time(&row.created_at)?,
            updated_at: parse_time(&row.updated_at)?,
            archived_at: row.archived_at.as_deref().map(parse_time).transpose()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use benchlab_core::{Accent, Project, ScenarioVariable};

    fn seed_project(store: &Store) -> Project {
        pollster::block_on(
            store.create_project(Project::new("Scenarios", "", Accent::Blue).unwrap()),
        )
        .unwrap()
    }

    #[test]
    fn scenario_saves_increment_revision_and_round_trip_steps() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let project = seed_project(&store);
            let mut scenario = Scenario::new(project.id, "Browse items");
            scenario
                .variables
                .push(ScenarioVariable::new("item_id", "7"));
            scenario.steps[0].name = "List items".to_owned();
            scenario.steps[0].url = "/items".to_owned();
            scenario.steps[0]
                .headers
                .push(benchlab_core::KeyValue::new("Accept", "application/json"));

            let saved = store.save_scenario(scenario).await.unwrap();
            assert_eq!(saved.revision, 1);
            assert_eq!(saved.steps[0].name, "List items");
            assert_eq!(saved.variables[0].value, "7");

            let mut edited = saved.clone();
            edited.steps[0].url = "/items/{{item_id}}".to_owned();
            let saved = store.save_scenario(edited).await.unwrap();
            assert_eq!(saved.revision, 2);
            assert_eq!(saved.steps[0].url, "/items/{{item_id}}");

            let loaded = store.get_scenario(saved.id).await.unwrap().unwrap();
            assert_eq!(loaded, saved);
        });
    }

    #[test]
    fn invalid_drafts_are_saved_but_names_are_required() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let project = seed_project(&store);
            let mut draft = Scenario::new(project.id, "Draft");
            draft.steps[0].url = String::new();
            draft.steps[0].expected_status.clear();
            let saved = store.save_scenario(draft).await.unwrap();
            assert_eq!(saved.steps[0].url, "");

            let mut nameless = saved.clone();
            nameless.name = "  ".to_owned();
            assert!(matches!(
                store.save_scenario(nameless).await,
                Err(StoreError::Validation(_))
            ));
        });
    }

    #[test]
    fn names_are_unique_archive_and_delete_round_trip() {
        pollster::block_on(async {
            let store = Store::open_in_memory().await.unwrap();
            let project = seed_project(&store);
            let first = store
                .save_scenario(Scenario::new(project.id, "Health"))
                .await
                .unwrap();
            let duplicate_name = Scenario::new(project.id, "health");
            assert!(matches!(
                store.save_scenario(duplicate_name).await,
                Err(StoreError::Conflict(_))
            ));

            let archived = store.archive_scenario(first.id, true).await.unwrap();
            assert!(archived.archived_at.is_some());
            assert!(
                store
                    .list_scenarios(project.id, false)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                store.list_scenarios(project.id, true).await.unwrap().len(),
                1
            );

            store.delete_scenario(first.id).await.unwrap();
            assert!(store.get_scenario(first.id).await.unwrap().is_none());
        });
    }
}
