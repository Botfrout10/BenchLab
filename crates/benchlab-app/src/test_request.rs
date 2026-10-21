//! Single-request tester used by the scenario editor (FR-SCN-15/16).
//!
//! Requests are sent from a background task. Secrets are resolved from the
//! keychain only for the lifetime of the request and every displayed value is
//! masked. Responses are returned to the UI in memory and never persisted.

use std::{
    collections::HashMap,
    io::Read,
    time::{Duration, Instant},
};

use base64::Engine as _;
use benchlab_core::{
    AuthConfig, Body, Environment, Id, ResponseFacts, Step, evaluate_assertions,
    evaluate_extractions, interpolate,
};
use benchlab_ui::backend::{
    BackendError, BackendResult, TestRequestPreview, TestStepRequest, TestStepResponse,
};
use reqwest::{blocking::Client, redirect::Policy};
use secrecy::ExposeSecret;
use url::Url;

use crate::services::AppServices;

const TRUNCATE_BYTES: usize = 256 * 1024;
const MASK: &str = "••••••";

pub async fn execute(
    services: &AppServices,
    request: TestStepRequest,
) -> BackendResult<TestStepResponse> {
    let environment = services
        .store
        .get_environment(request.environment_id)
        .await
        .map_err(map_store_error)?
        .ok_or_else(|| BackendError::new("That environment no longer exists."))?;
    if environment.archived_at.is_some() {
        return Err(BackendError::new(
            "This environment is archived. Restore it before testing a step.",
        ));
    }
    if let Some(project_id) = services
        .store
        .get_scenario(request.scenario.id)
        .await
        .map_err(map_store_error)?
        .map(|scenario| scenario.project_id)
        && project_id != environment.project_id
    {
        return Err(BackendError::new(
            "The selected environment belongs to a different project.",
        ));
    }

    let step = request
        .scenario
        .steps
        .get(request.step_index)
        .cloned()
        .ok_or_else(|| BackendError::new("Select a step to test."))?;

    let mut validation_scenario = request.scenario.clone();
    for extra in &request.extra_variables {
        if !validation_scenario
            .variables
            .iter()
            .any(|variable| variable.key == extra.key)
        {
            validation_scenario.variables.push(extra.clone());
        }
    }
    let errors = validation_scenario.validate_step(request.step_index, Some(&environment));
    if !errors.is_empty() {
        return Err(BackendError {
            message: "Fix this step's validation errors before testing it.".to_owned(),
            fields: errors,
        });
    }

    let (mut variables, secret_values) = resolve_variables(services, &environment)?;
    for variable in &validation_scenario.variables {
        variables.insert(variable.key.clone(), variable.value.clone());
    }
    variables.insert("base_url".to_owned(), environment.base_url.clone());
    variables.insert(
        "run_id".to_owned(),
        format!("test-{}", &Id::now_v7().simple().to_string()[..8]),
    );

    send_request(
        &environment,
        &step,
        request.step_index,
        &variables,
        &secret_values,
    )
}

fn resolve_variables(
    services: &AppServices,
    environment: &Environment,
) -> BackendResult<(HashMap<String, String>, Vec<String>)> {
    let mut variables = HashMap::new();
    let mut secrets = Vec::new();
    for variable in &environment.variables {
        let value = if variable.secret {
            let secret_ref = variable.secret_ref.ok_or_else(|| {
                BackendError::new(format!(
                    "Secret variable {} has no keychain reference.",
                    variable.key
                ))
            })?;
            let value = services
                .secrets
                .get(secret_ref)
                .map_err(|error| BackendError::new(format!("Secret storage failed: {error}")))?
                .ok_or_else(|| {
                    BackendError::new(format!(
                        "Secret variable {} is unavailable. Re-enter it in the environment.",
                        variable.key
                    ))
                })?;
            let value = value.expose_secret().to_owned();
            if !value.is_empty() {
                secrets.push(value.clone());
            }
            value
        } else {
            variable.value.clone().unwrap_or_default()
        };
        variables.insert(variable.key.clone(), value);
    }
    // Longest values first so overlapping secrets are fully masked.
    secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
    secrets.dedup();
    Ok((variables, secrets))
}

fn send_request(
    environment: &Environment,
    step: &Step,
    step_index: usize,
    variables: &HashMap<String, String>,
    secret_values: &[String],
) -> BackendResult<TestStepResponse> {
    let field = |name: &str| format!("steps[{step_index}].{name}");
    let mut url = resolve_url(environment, step, &field("url"), variables)?;
    for parameter in &step.query {
        if parameter.is_blank() {
            continue;
        }
        let key = interpolate(&parameter.key, variables)
            .map_err(|message| BackendError::field(message, field("query")))?;
        let value = interpolate(&parameter.value, variables)
            .map_err(|message| BackendError::field(message, field("query")))?;
        if has_crlf(&key) || has_crlf(&value) {
            return Err(BackendError::field(
                "Query parameters cannot contain line breaks (CR/LF).",
                field("query"),
            ));
        }
        url.query_pairs_mut().append_pair(&key, &value);
    }
    if !matches!(url.scheme(), "http" | "https") {
        return Err(BackendError::field(
            "URL must use http or https.",
            field("url"),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(BackendError::field(
            "URLs must not contain credentials. Put secrets in headers.",
            field("url"),
        ));
    }
    if !host_allowed(environment, &url) {
        return Err(BackendError::field(
            format!(
                "Host “{}” is not allowed by this environment.",
                url.host_str().unwrap_or("unknown")
            ),
            field("url"),
        ));
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    for (index, header) in step.headers.iter().enumerate() {
        if header.is_blank() {
            continue;
        }
        let key = interpolate(&header.key, variables)
            .map_err(|message| BackendError::field(message, field(&format!("headers[{index}]"))))?;
        let value = interpolate(&header.value, variables)
            .map_err(|message| BackendError::field(message, field(&format!("headers[{index}]"))))?;
        if has_crlf(&key) || has_crlf(&value) {
            return Err(BackendError::field(
                "Header names and values cannot contain line breaks (CR/LF).",
                field(&format!("headers[{index}]")),
            ));
        }
        if key.eq_ignore_ascii_case("host") {
            return Err(BackendError::field(
                "The Host header is controlled by the request URL.",
                field(&format!("headers[{index}].key")),
            ));
        }
        headers.push((key, value));
    }

    let body = step
        .body
        .template()
        .map(|template| {
            interpolate(template, variables)
                .map_err(|message| BackendError::field(message, field("body")))
        })
        .transpose()?;

    if let Some(body) = body.as_ref() {
        let existing = headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"));
        if !existing {
            let content_type = match step.body {
                Body::Json { .. } => "application/json",
                Body::Text { .. } => "text/plain; charset=utf-8",
                Body::None => "",
            };
            if !content_type.is_empty() {
                headers.push(("Content-Type".to_owned(), content_type.to_owned()));
            }
        }
        if match step.body {
            Body::Json { .. } => serde_json::from_str::<serde_json::Value>(body).is_err(),
            _ => false,
        } {
            return Err(BackendError::field(
                "JSON body is not valid after variables are resolved. Check for unquoted string values.",
                field("body"),
            ));
        }
    }

    apply_auth(environment, &mut headers, variables, &field)?;

    let request_preview = TestRequestPreview {
        method: step.method,
        url: mask(url.as_str(), secret_values),
        headers: headers
            .iter()
            .map(|(name, value)| (name.clone(), mask(value, secret_values)))
            .collect(),
        body: body.as_ref().map(|body| mask(body, secret_values)),
    };

    let mut builder = Client::builder()
        .timeout(Duration::from_millis(step.timeout_ms))
        .connect_timeout(Duration::from_millis(step.timeout_ms.min(10_000)))
        .user_agent(format!("BenchLab/{}", benchlab_core::APP_VERSION))
        .redirect(redirect_policy(environment));
    if !environment.tls_verify {
        builder = builder.danger_accept_invalid_certs(true);
    }
    let client = builder.build().map_err(|error| {
        BackendError::new(format!("Could not prepare the HTTP client: {error}"))
    })?;

    let request_method = reqwest::Method::from_bytes(step.method.as_str().as_bytes())
        .map_err(|_| BackendError::new("Unsupported HTTP method."))?;
    let mut request = client.request(request_method, url.clone());
    for (name, value) in &headers {
        request = request.header(name, value);
    }
    if let Some(body) = body.as_ref() {
        request = request.body(body.clone());
    }

    tracing::info!(
        method = step.method.as_str(),
        host = url.host_str().unwrap_or("unknown"),
        "sending single-request test"
    );
    let started = Instant::now();
    let response = request.send().map_err(map_request_error)?;
    let status = response.status().as_u16();
    let content_length = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let response_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or("<binary value>").to_owned(),
            )
        })
        .collect();
    let content_type = response_header(&response_headers, "content-type")
        .unwrap_or_default()
        .to_owned();

    let mut bytes = Vec::new();
    response
        .take((TRUNCATE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| BackendError::new(format!("Could not read the response body: {error}")))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    let truncated = bytes.len() > TRUNCATE_BYTES;
    if truncated {
        bytes.truncate(TRUNCATE_BYTES);
    }
    let size_bytes = content_length.unwrap_or(bytes.len() as u64);
    let raw_body = String::from_utf8_lossy(&bytes).into_owned();
    let display_body = if truncated {
        raw_body.clone()
    } else {
        format_body(&content_type, &raw_body)
    };

    let response_facts = ResponseFacts {
        status,
        headers: &response_headers,
        body: &raw_body,
        duration_ms,
        size_bytes,
    };
    let mut assertion_results = evaluate_assertions(
        &step.expected_status,
        &step.assertions,
        &response_facts,
        variables,
    );
    for result in &mut assertion_results {
        result.label = mask(&result.label, secret_values);
        result.detail = mask(&result.detail, secret_values);
    }
    let mut extraction_results =
        evaluate_extractions(&step.extract, status, &response_headers, &raw_body);
    for result in &mut extraction_results {
        if let Some(value) = &mut result.value {
            *value = mask(value, secret_values);
        }
    }

    Ok(TestStepResponse {
        status,
        duration_ms,
        size_bytes,
        truncated,
        headers: response_headers
            .into_iter()
            .map(|(name, value)| (name, mask(&value, secret_values)))
            .collect(),
        body: mask(&display_body, secret_values),
        request: request_preview,
        assertions: assertion_results,
        extractions: extraction_results,
    })
}

fn resolve_url(
    environment: &Environment,
    step: &Step,
    field: &str,
    variables: &HashMap<String, String>,
) -> BackendResult<Url> {
    let resolved =
        interpolate(&step.url, variables).map_err(|message| BackendError::field(message, field))?;
    if has_crlf(&resolved) {
        return Err(BackendError::field(
            "URLs cannot contain line breaks (CR/LF).",
            field,
        ));
    }
    let base = Url::parse(&environment.base_url).map_err(|_| {
        BackendError::new("The environment base URL is invalid. Fix it before testing.")
    })?;
    if resolved.contains("://") {
        Url::parse(&resolved).map_err(|_| BackendError::field("Enter a valid absolute URL.", field))
    } else {
        base.join(&resolved)
            .map_err(|_| BackendError::field("Enter a valid relative URL.", field))
    }
}

fn apply_auth(
    environment: &Environment,
    headers: &mut Vec<(String, String)>,
    variables: &HashMap<String, String>,
    field: &impl Fn(&str) -> String,
) -> BackendResult<()> {
    let (name, value) = match &environment.auth {
        AuthConfig::None => return Ok(()),
        AuthConfig::Bearer { token_var } => {
            let token = variables.get(token_var).ok_or_else(|| {
                BackendError::field(
                    format!("Auth variable “{token_var}” is not defined."),
                    field("auth"),
                )
            })?;
            ("Authorization".to_owned(), format!("Bearer {token}"))
        }
        AuthConfig::Basic {
            username_var,
            password_var,
        } => {
            let username = variables.get(username_var).ok_or_else(|| {
                BackendError::field(
                    format!("Auth variable “{username_var}” is not defined."),
                    field("auth"),
                )
            })?;
            let password = variables.get(password_var).ok_or_else(|| {
                BackendError::field(
                    format!("Auth variable “{password_var}” is not defined."),
                    field("auth"),
                )
            })?;
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
            ("Authorization".to_owned(), format!("Basic {encoded}"))
        }
        AuthConfig::Header { name, value_var } => {
            let value = variables.get(value_var).ok_or_else(|| {
                BackendError::field(
                    format!("Auth variable “{value_var}” is not defined."),
                    field("auth"),
                )
            })?;
            (name.clone(), value.clone())
        }
    };
    if has_crlf(&name) || has_crlf(&value) {
        return Err(BackendError::new(
            "Resolved authentication contains a line break and was not sent.",
        ));
    }
    headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(&name));
    headers.push((name, value));
    Ok(())
}

fn redirect_policy(environment: &Environment) -> Policy {
    let mut owned = environment.clone();
    owned.archived_at = None;
    Policy::custom(move |attempt| {
        if attempt.previous().len() >= 10 {
            attempt.stop()
        } else if host_allowed(&owned, attempt.url()) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

fn host_allowed(environment: &Environment, url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let port = url.port_or_known_default();
    let base_url = Url::parse(&environment.base_url).ok();
    let base_host = base_url.as_ref().and_then(Url::host_str);
    let base_port = base_url.as_ref().and_then(Url::port_or_known_default);
    if base_host.is_some_and(|base| base.eq_ignore_ascii_case(host)) && base_port == port {
        return true;
    }
    if !environment.allow_external_hosts {
        return false;
    }
    let host_port = port.map(|port| format!("{host}:{port}"));
    environment.allowed_hosts.iter().any(|allowed| {
        let allowed = allowed.trim();
        allowed.eq_ignore_ascii_case(host)
            || host_port
                .as_deref()
                .is_some_and(|host_port| allowed.eq_ignore_ascii_case(host_port))
            || allowed
                .parse::<Url>()
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .is_some_and(|allowed_host| allowed_host.eq_ignore_ascii_case(host))
    })
}

fn response_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn format_body(content_type: &str, body: &str) -> String {
    let trimmed = body.trim_start();
    if (content_type.to_ascii_lowercase().contains("json")
        || trimmed.starts_with('{')
        || trimmed.starts_with('['))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(body)
    {
        return serde_json::to_string_pretty(&value).unwrap_or_else(|_| body.to_owned());
    }
    body.to_owned()
}

fn mask(value: &str, secrets: &[String]) -> String {
    let mut masked = value.to_owned();
    for secret in secrets {
        if !secret.is_empty() {
            masked = masked.replace(secret, MASK);
        }
    }
    masked
}

fn has_crlf(value: &str) -> bool {
    value.contains(['\r', '\n'])
}

fn map_request_error(error: reqwest::Error) -> BackendError {
    if error.is_timeout() {
        BackendError::new("The request timed out before a response completed.")
    } else if error.is_connect() {
        BackendError::new(
            "Could not connect to the target. Check the environment base URL and that the server is running.",
        )
    } else {
        BackendError::new(format!("The request failed: {error}"))
    }
}

fn map_store_error(error: benchlab_store::StoreError) -> BackendError {
    BackendError::new(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockTarget;
    use crate::services::AppServices;
    use benchlab_core::{Accent, EnvironmentKind, Project, Scenario, ScenarioVariable, Variable};
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
            engine: benchlab_core::EngineStatus {
                found: true,
                path: None,
                version: Some("2.2.0".to_owned()),
                supported: true,
                message: "ready".to_owned(),
            },
            mock: Arc::new(crate::mock::MockTargetManager::default()),
        };
        (services, temp)
    }

    #[test]
    fn test_step_resolves_variables_auth_extracts_and_masks_secrets() {
        let target = MockTarget::start().unwrap();
        let (services, _temp) = services();
        let project = pollster::block_on(
            services
                .store
                .create_project(Project::new("Test", "", Accent::Blue).unwrap()),
        )
        .unwrap();
        let secret_ref = Id::now_v7();
        services
            .secrets
            .set(secret_ref, secrecy::SecretString::from("super-secret"))
            .unwrap();
        let mut environment = Environment::new(
            project.id,
            "Local",
            EnvironmentKind::Local,
            target.base_url(),
        )
        .unwrap();
        environment.variables.push(Variable::plain("item_id", "1"));
        environment
            .variables
            .push(Variable::secret("api_key", secret_ref));
        environment.auth = AuthConfig::Bearer {
            token_var: "api_key".to_owned(),
        };
        let environment = pollster::block_on(services.store.save_environment(environment)).unwrap();

        let mut scenario = Scenario::new(project.id, "Items");
        scenario.steps[0].name = "Fetch".to_owned();
        scenario.steps[0].url = "{{base_url}}/items/{{item_id}}".to_owned();
        scenario.steps[0]
            .headers
            .push(benchlab_core::KeyValue::new("X-Secret", "{{api_key}}"));
        scenario.steps[0]
            .extract
            .push(benchlab_core::Extraction::JsonPath {
                variable: "name".to_owned(),
                path: "$.name".to_owned(),
            });
        let response = pollster::block_on(execute(
            &services,
            TestStepRequest {
                scenario,
                step_index: 0,
                environment_id: environment.id,
                extra_variables: vec![ScenarioVariable::new("unused", "x")],
            },
        ))
        .unwrap();
        assert_eq!(response.status, 200);
        assert!(response.body.contains("\"name\": \"Item 1\""));
        assert_eq!(response.extractions[0].value.as_deref(), Some("Item 1"));
        assert_eq!(
            response
                .request
                .headers
                .iter()
                .find(|(name, _)| name == "Authorization")
                .unwrap()
                .1,
            "Bearer ••••••"
        );
        assert!(!response.body.contains("super-secret"));
        assert!(
            !response
                .request
                .headers
                .iter()
                .any(|(_, value)| value.contains("super-secret"))
        );
        assert!(response.assertions.iter().all(|result| result.passed));
    }

    #[test]
    fn host_pinning_blocks_external_targets() {
        let (services, _temp) = services();
        let project = pollster::block_on(
            services
                .store
                .create_project(Project::new("Test", "", Accent::Blue).unwrap()),
        )
        .unwrap();
        let environment = Environment::new(
            project.id,
            "Local",
            EnvironmentKind::Local,
            "http://127.0.0.1:1",
        )
        .unwrap();
        let environment = pollster::block_on(services.store.save_environment(environment)).unwrap();
        let mut scenario = Scenario::new(project.id, "External");
        scenario.steps[0].url = "http://example.com/".to_owned();
        let error = pollster::block_on(execute(
            &services,
            TestStepRequest {
                scenario,
                step_index: 0,
                environment_id: environment.id,
                extra_variables: Vec::new(),
            },
        ))
        .unwrap_err();
        assert!(
            error.message.contains("not allowed")
                || error
                    .fields
                    .iter()
                    .any(|e| e.message.contains("not allowed"))
        );
    }
}
