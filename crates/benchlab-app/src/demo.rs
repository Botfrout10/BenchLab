//! Bundled demo project seed (FR-DEMO-01/02). The seed is idempotent: if the
//! demo project already exists it is opened as-is. Missing demo pieces are
//! recreated, but user edits are never overwritten.

use benchlab_core::{
    Accent, Assertion, Body, Environment, EnvironmentKind, Extraction, Id, KeyValue, Project,
    Scenario, Step, ThinkTime,
};
use benchlab_ui::backend::{BackendError, BackendResult, DemoView};
use serde_json::Value;

use crate::services::AppServices;

pub const DEMO_PROJECT_NAME: &str = "BenchLab Demo";
pub const DEMO_PROJECT_DESCRIPTION: &str =
    "A three-step scenario against BenchLab's deterministic local mock target.";
pub const DEMO_ENVIRONMENT_NAME: &str = "Local mock target";
pub const DEMO_SCENARIO_NAME: &str = "Demo: items and echo";
const DEMO_PROJECT_SETTING: &str = "demo.project_id";
const DEMO_SCENARIO_SETTING: &str = "demo.scenario_id";

pub async fn open_demo(services: &AppServices) -> BackendResult<DemoView> {
    let base_url = services
        .mock
        .ensure()
        .map_err(|error| BackendError::new(format!("Could not start the demo target: {error}")))?;

    let project = match demo_project_id(services).await? {
        Some(id) => match services
            .store
            .get_project(id)
            .await
            .map_err(map_store_error)?
        {
            Some(mut project) => {
                if project.archived_at.is_some() {
                    project = services
                        .store
                        .archive_project(project.id, false)
                        .await
                        .map_err(map_store_error)?;
                }
                project
            }
            None => create_demo_project(services).await?,
        },
        None => {
            if services
                .store
                .project_name_exists(DEMO_PROJECT_NAME, None)
                .await
                .map_err(map_store_error)?
                .is_some()
            {
                return Err(BackendError::new(format!(
                    "A project named “{DEMO_PROJECT_NAME}” already exists but is not the BenchLab demo. Archive or rename it, then try again."
                )));
            }
            create_demo_project(services).await?
        }
    };
    services
        .store
        .set_setting(DEMO_PROJECT_SETTING, Value::String(project.id.to_string()))
        .await
        .map_err(map_store_error)?;

    let environment = ensure_environment(services, &project, &base_url).await?;
    let scenario = ensure_scenario(services, &project, &environment).await?;
    services
        .store
        .set_setting(
            DEMO_SCENARIO_SETTING,
            Value::String(scenario.id.to_string()),
        )
        .await
        .map_err(map_store_error)?;

    Ok(DemoView {
        project_id: project.id,
        scenario_id: scenario.id,
        base_url,
    })
}

async fn demo_project_id(services: &AppServices) -> BackendResult<Option<Id>> {
    match services
        .store
        .get_setting(DEMO_PROJECT_SETTING)
        .await
        .map_err(map_store_error)?
    {
        Some(Value::String(value)) => Id::parse_str(&value).map(Some).map_err(|error| {
            BackendError::new(format!("The demo project setting is invalid: {error}"))
        }),
        Some(_) => Err(BackendError::new(
            "The demo project setting has an unexpected type.",
        )),
        None => Ok(None),
    }
}

async fn demo_scenario_id(services: &AppServices) -> BackendResult<Option<Id>> {
    match services
        .store
        .get_setting(DEMO_SCENARIO_SETTING)
        .await
        .map_err(map_store_error)?
    {
        Some(Value::String(value)) => Id::parse_str(&value).map(Some).map_err(|error| {
            BackendError::new(format!("The demo scenario setting is invalid: {error}"))
        }),
        Some(_) => Err(BackendError::new(
            "The demo scenario setting has an unexpected type.",
        )),
        None => Ok(None),
    }
}

async fn create_demo_project(services: &AppServices) -> BackendResult<Project> {
    let project = Project::new(DEMO_PROJECT_NAME, DEMO_PROJECT_DESCRIPTION, Accent::Teal)
        .map_err(validation_error)?;
    services
        .store
        .create_project(project)
        .await
        .map_err(map_store_error)
}

async fn ensure_environment(
    services: &AppServices,
    project: &Project,
    base_url: &str,
) -> BackendResult<Environment> {
    let environments = services
        .store
        .list_environments(project.id, true)
        .await
        .map_err(map_store_error)?;
    if let Some(environment) = environments
        .into_iter()
        .find(|environment| environment.name == DEMO_ENVIRONMENT_NAME)
    {
        let mut environment = environment;
        let mut changed = environment.archived_at.is_some();
        environment.archived_at = None;
        if environment.base_url != base_url {
            environment.base_url = base_url.to_owned();
            changed = true;
        }
        if changed {
            return services
                .store
                .save_environment(environment)
                .await
                .map_err(map_store_error);
        }
        return Ok(environment);
    }

    let environment = Environment::new(
        project.id,
        DEMO_ENVIRONMENT_NAME,
        EnvironmentKind::Local,
        base_url,
    )
    .map_err(validation_error)?;
    services
        .store
        .save_environment(environment)
        .await
        .map_err(map_store_error)
}

async fn ensure_scenario(
    services: &AppServices,
    project: &Project,
    environment: &Environment,
) -> BackendResult<Scenario> {
    if let Some(id) = demo_scenario_id(services).await?
        && let Some(mut scenario) = services
            .store
            .get_scenario(id)
            .await
            .map_err(map_store_error)?
        && scenario.project_id == project.id
    {
        if scenario.archived_at.is_some() {
            scenario = services
                .store
                .archive_scenario(scenario.id, false)
                .await
                .map_err(map_store_error)?;
        }
        return Ok(scenario);
    }

    if let Some(scenario) = services
        .store
        .list_scenarios(project.id, true)
        .await
        .map_err(map_store_error)?
        .into_iter()
        .find(|scenario| scenario.name == DEMO_SCENARIO_NAME)
    {
        return Ok(scenario);
    }

    let scenario = demo_scenario(project.id);
    let errors = scenario.validate_with_environment(Some(environment));
    if !errors.is_empty() {
        let detail = errors
            .iter()
            .map(|error| format!("{}: {}", error.field, error.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(BackendError::new(format!(
            "The bundled demo scenario failed validation: {detail}"
        )));
    }
    services
        .store
        .save_scenario(scenario)
        .await
        .map_err(map_store_error)
}

fn demo_scenario(project_id: Id) -> Scenario {
    let mut scenario = Scenario::new(project_id, DEMO_SCENARIO_NAME);
    scenario.description =
        "List items, fetch the first extracted item, then echo it back.".to_owned();

    let mut list = Step::blank(0);
    list.name = "List items".to_owned();
    list.url = "{{base_url}}/items".to_owned();
    list.headers
        .push(KeyValue::new("Accept", "application/json"));
    list.think_time = ThinkTime::Fixed { ms: 50 };
    list.assertions.push(Assertion::BodyContains {
        value: "\"id\":1".to_owned(),
    });
    list.assertions
        .push(Assertion::MaxDurationMs { value: 2_000 });
    list.extract.push(Extraction::JsonPath {
        variable: "item_id".to_owned(),
        path: "$[0].id".to_owned(),
    });
    list.extract.push(Extraction::JsonPath {
        variable: "item_name".to_owned(),
        path: "$[0].name".to_owned(),
    });

    let mut fetch = Step::blank(1);
    fetch.name = "Fetch item".to_owned();
    fetch.url = "{{base_url}}/items/{{item_id}}".to_owned();
    fetch.think_time = ThinkTime::Range {
        min_ms: 10,
        max_ms: 20,
    };
    fetch.assertions.push(Assertion::JsonPathEquals {
        path: "$.id".to_owned(),
        value: Value::from(1),
    });
    fetch.assertions.push(Assertion::JsonPathEquals {
        path: "$.name".to_owned(),
        value: Value::from("Item 1"),
    });
    fetch.assertions.push(Assertion::HeaderEquals {
        name: "content-type".to_owned(),
        value: "application/json".to_owned(),
    });
    fetch.extract.push(Extraction::JsonPath {
        variable: "fetched_name".to_owned(),
        path: "$.name".to_owned(),
    });

    let mut echo = Step::blank(2);
    echo.name = "Echo item".to_owned();
    echo.method = benchlab_core::Method::Post;
    echo.url = "{{base_url}}/echo".to_owned();
    echo.body = Body::Json {
        template:
            r#"{"item_id": {{item_id}}, "name": "{{fetched_name}}", "source": "benchlab-demo"}"#
                .to_owned(),
    };
    echo.assertions.push(Assertion::BodyRegex {
        pattern: r#""item_id":\s*1"#.to_owned(),
    });
    echo.assertions.push(Assertion::BodyContains {
        value: "\"name\":\"Item 1\"".to_owned(),
    });
    echo.extract.push(Extraction::Status {
        variable: "echo_status".to_owned(),
    });

    scenario.steps = vec![list, fetch, echo];
    scenario
}

fn validation_error(errors: Vec<benchlab_core::FieldError>) -> BackendError {
    BackendError {
        message: "The bundled demo data failed validation.".to_owned(),
        fields: errors,
    }
}

fn map_store_error(error: benchlab_store::StoreError) -> BackendError {
    BackendError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockTargetManager;
    use benchlab_core::EngineStatus;
    use benchlab_store::{AppPaths, SessionSecretStore, Store};
    use std::sync::Arc;

    fn services() -> (AppServices, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(temp.path());
        let store = pollster::block_on(Store::open(paths.db_path())).unwrap();
        let services = AppServices {
            paths,
            store,
            secrets: Arc::new(SessionSecretStore::default()),
            engine: EngineStatus {
                found: true,
                path: None,
                version: Some("2.2.0".to_owned()),
                supported: true,
                message: "ready".to_owned(),
            },
            mock: Arc::new(MockTargetManager::default()),
        };
        (services, temp)
    }

    #[test]
    fn gate_c_demo_three_step_scenario_runs_against_mock_target() {
        use crate::test_request::execute;
        use benchlab_core::ScenarioVariable;
        use benchlab_ui::backend::TestStepRequest;

        let (services, _temp) = services();
        let demo = pollster::block_on(open_demo(&services)).unwrap();
        let scenario = pollster::block_on(services.store.get_scenario(demo.scenario_id))
            .unwrap()
            .unwrap();
        assert_eq!(scenario.steps.len(), 3);

        let environment =
            pollster::block_on(services.store.list_environments(demo.project_id, false))
                .unwrap()
                .into_iter()
                .find(|environment| environment.name == DEMO_ENVIRONMENT_NAME)
                .expect("demo environment");

        let mut extra_variables: Vec<ScenarioVariable> = Vec::new();
        let mut tested = Vec::new();

        for (step_index, step) in scenario.steps.iter().enumerate() {
            let response = pollster::block_on(execute(
                &services,
                TestStepRequest {
                    scenario: scenario.clone(),
                    step_index,
                    environment_id: environment.id,
                    extra_variables: extra_variables.clone(),
                },
            ))
            .unwrap_or_else(|error| {
                panic!(
                    "step {} ({}) failed to run: {}",
                    step_index + 1,
                    step.name,
                    error.message
                )
            });

            assert_eq!(
                response.status,
                200,
                "step {} ({}) returned HTTP {}",
                step_index + 1,
                step.name,
                response.status
            );
            let failed: Vec<_> = response
                .assertions
                .iter()
                .filter(|assertion| !assertion.passed)
                .map(|assertion| format!("{}: {}", assertion.label, assertion.detail))
                .collect();
            assert!(
                failed.is_empty(),
                "step {} ({}) failed assertions: {}",
                step_index + 1,
                step.name,
                failed.join("; ")
            );

            for extraction in &response.extractions {
                let Some(value) = extraction.value.clone() else {
                    panic!(
                        "step {} ({}) could not extract {}: {}",
                        step_index + 1,
                        step.name,
                        extraction.variable,
                        extraction.error.as_deref().unwrap_or("no value")
                    );
                };
                match extra_variables
                    .iter_mut()
                    .find(|variable| variable.key == extraction.variable)
                {
                    Some(existing) => existing.value = value,
                    None => extra_variables
                        .push(ScenarioVariable::new(extraction.variable.clone(), value)),
                }
            }

            tested.push(response);
        }

        let value = |key: &str| {
            extra_variables
                .iter()
                .find(|variable| variable.key == key)
                .map(|variable| variable.value.as_str())
                .unwrap_or_else(|| panic!("missing extracted variable {key}"))
        };
        assert_eq!(value("item_id"), "1");
        assert_eq!(value("item_name"), "Item 1");
        assert_eq!(value("fetched_name"), "Item 1");
        assert_eq!(value("echo_status"), "200");
        assert!(tested[2].body.contains("\"source\": \"benchlab-demo\""));
    }

    #[test]
    fn demo_is_idempotent_and_edit_preserving() {
        let (services, _temp) = services();
        let first = pollster::block_on(open_demo(&services)).unwrap();
        let second = pollster::block_on(open_demo(&services)).unwrap();
        assert_eq!(first.project_id, second.project_id);
        assert_eq!(first.scenario_id, second.scenario_id);
        assert_eq!(first.base_url, second.base_url);

        let mut scenario = pollster::block_on(services.store.get_scenario(first.scenario_id))
            .unwrap()
            .unwrap();
        scenario.description = "edited by the user".to_owned();
        pollster::block_on(services.store.save_scenario(scenario)).unwrap();

        let third = pollster::block_on(open_demo(&services)).unwrap();
        let scenario = pollster::block_on(services.store.get_scenario(third.scenario_id))
            .unwrap()
            .unwrap();
        assert_eq!(scenario.description, "edited by the user");
    }
}
