//! Scenario definitions, static validation, variable interpolation, and
//! request/response assertion helpers (SPEC.md §6.3 and §10.1).

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use crate::{Environment, FieldError, Id, is_header_name, now, validate_variable_key};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl Method {
    pub const ALL: [Method; 7] = [
        Method::Get,
        Method::Post,
        Method::Put,
        Method::Patch,
        Method::Delete,
        Method::Head,
        Method::Options,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
            Method::Head => "HEAD",
            Method::Options => "OPTIONS",
        }
    }

    pub fn allows_body(self) -> bool {
        !matches!(self, Method::Get | Method::Head)
    }
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    pub fn is_blank(&self) -> bool {
        self.key.trim().is_empty() && self.value.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    #[default]
    None,
    Json {
        template: String,
    },
    Text {
        template: String,
    },
}

impl Body {
    pub fn template(&self) -> Option<&str> {
        match self {
            Body::None => None,
            Body::Json { template } | Body::Text { template } => Some(template),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Body::None => "None",
            Body::Json { .. } => "JSON",
            Body::Text { .. } => "Raw text",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ThinkTime {
    #[default]
    None,
    Fixed {
        ms: u64,
    },
    Range {
        min_ms: u64,
        max_ms: u64,
    },
}

impl ThinkTime {
    pub fn label(&self) -> &'static str {
        match self {
            ThinkTime::None => "None",
            ThinkTime::Fixed { .. } => "Fixed",
            ThinkTime::Range { .. } => "Range",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Assertion {
    StatusIn { values: Vec<u16> },
    HeaderEquals { name: String, value: String },
    BodyContains { value: String },
    BodyRegex { pattern: String },
    JsonPathEquals { path: String, value: Value },
    MaxDurationMs { value: u64 },
    MaxSizeBytes { value: u64 },
}

impl Assertion {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Assertion::StatusIn { .. } => "Status",
            Assertion::HeaderEquals { .. } => "Header equals",
            Assertion::BodyContains { .. } => "Body contains",
            Assertion::BodyRegex { .. } => "Body regex",
            Assertion::JsonPathEquals { .. } => "JSON path equals",
            Assertion::MaxDurationMs { .. } => "Max duration",
            Assertion::MaxSizeBytes { .. } => "Max size",
        }
    }

    pub fn label(&self) -> String {
        match self {
            Assertion::StatusIn { values } => {
                format!("status in {}", join_statuses(values))
            }
            Assertion::HeaderEquals { name, value } => format!("{name} equals {value}"),
            Assertion::BodyContains { .. } => "body contains text".to_owned(),
            Assertion::BodyRegex { pattern } => format!("body matches /{pattern}/"),
            Assertion::JsonPathEquals { path, value } => {
                format!("{path} equals {}", json_scalar_text(value))
            }
            Assertion::MaxDurationMs { value } => format!("duration ≤ {value} ms"),
            Assertion::MaxSizeBytes { value } => format!("size ≤ {value} bytes"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Extraction {
    JsonPath {
        variable: String,
        path: String,
    },
    Regex {
        variable: String,
        pattern: String,
        group: u8,
    },
    Header {
        variable: String,
        name: String,
    },
    Status {
        variable: String,
    },
}

impl Extraction {
    pub fn kind_label(&self) -> &'static str {
        match self {
            Extraction::JsonPath { .. } => "JSON path",
            Extraction::Regex { .. } => "Regex group",
            Extraction::Header { .. } => "Header",
            Extraction::Status { .. } => "Status",
        }
    }

    pub fn variable(&self) -> &str {
        match self {
            Extraction::JsonPath { variable, .. }
            | Extraction::Regex { variable, .. }
            | Extraction::Header { variable, .. }
            | Extraction::Status { variable } => variable,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioVariable {
    pub key: String,
    pub value: String,
}

impl ScenarioVariable {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub id: Id,
    pub name: String,
    pub method: Method,
    /// Relative to the environment base URL, or absolute. `{{vars}}` allowed.
    pub url: String,
    pub headers: Vec<KeyValue>,
    pub query: Vec<KeyValue>,
    pub body: Body,
    pub timeout_ms: u64,
    pub think_time: ThinkTime,
    pub expected_status: Vec<u16>,
    pub assertions: Vec<Assertion>,
    pub extract: Vec<Extraction>,
}

impl Step {
    pub fn blank(index: usize) -> Self {
        Self {
            id: Id::now_v7(),
            name: format!("Request {}", index + 1),
            method: Method::Get,
            url: "/".to_owned(),
            headers: Vec::new(),
            query: Vec::new(),
            body: Body::None,
            timeout_ms: 30_000,
            think_time: ThinkTime::None,
            expected_status: vec![200],
            assertions: Vec::new(),
            extract: Vec::new(),
        }
    }

    pub fn assertion_count(&self) -> usize {
        (!self.expected_status.is_empty()) as usize + self.assertions.len()
    }

    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_owned();
        self.url = self.url.trim().to_owned();
        self.headers.retain(|row| !row.is_blank());
        self.query.retain(|row| !row.is_blank());
        for row in &mut self.headers {
            row.key = row.key.trim().to_owned();
            row.value = row.value.trim().to_owned();
        }
        for row in &mut self.query {
            row.key = row.key.trim().to_owned();
        }
        self.expected_status.sort_unstable();
        self.expected_status.dedup();
    }

    fn validate(&self, path: &str, context: &VariableContext) -> Vec<FieldError> {
        let mut errors = Vec::new();
        let name = self.name.trim();
        if name.is_empty() {
            errors.push(FieldError::new(
                format!("{path}.name"),
                "Step name is required.",
            ));
        } else if name.chars().count() > 80 {
            errors.push(FieldError::new(
                format!("{path}.name"),
                "Step name must be 80 characters or fewer.",
            ));
        }

        if self.url.trim().is_empty() {
            errors.push(FieldError::new(
                format!("{path}.url"),
                "Request URL is required.",
            ));
        } else {
            errors.extend(validate_url_template(
                &self.url,
                &format!("{path}.url"),
                context,
            ));
        }

        let mut header_names = HashSet::new();
        for (index, header) in self.headers.iter().enumerate() {
            let field = format!("{path}.headers[{index}]");
            if header.is_blank() {
                continue;
            }
            let key = header.key.trim();
            if key.is_empty() {
                errors.push(FieldError::new(
                    format!("{field}.key"),
                    "Header name is required.",
                ));
            } else if !is_header_name(key) {
                errors.push(FieldError::new(
                    format!("{field}.key"),
                    "Header name contains invalid characters.",
                ));
            } else if !header_names.insert(key.to_ascii_lowercase()) {
                errors.push(FieldError::new(
                    format!("{field}.key"),
                    "Header names must be unique within a step.",
                ));
            }
            if contains_crlf(&header.value) {
                errors.push(FieldError::new(
                    format!("{field}.value"),
                    "Header values cannot contain line breaks (CR/LF).",
                ));
            } else {
                errors.extend(validate_template_field(
                    &header.value,
                    &format!("{field}.value"),
                    context,
                    true,
                ));
            }
        }

        for (index, query) in self.query.iter().enumerate() {
            let field = format!("{path}.query[{index}]");
            if query.is_blank() {
                continue;
            }
            if query.key.trim().is_empty() {
                errors.push(FieldError::new(
                    format!("{field}.key"),
                    "Query parameter name is required.",
                ));
            } else if contains_crlf(&query.key) {
                errors.push(FieldError::new(
                    format!("{field}.key"),
                    "Query parameter names cannot contain line breaks (CR/LF).",
                ));
            } else {
                errors.extend(validate_url_value(
                    &query.key,
                    &format!("{field}.key"),
                    context,
                ));
            }
            if contains_crlf(&query.value) {
                errors.push(FieldError::new(
                    format!("{field}.value"),
                    "Query parameter values cannot contain line breaks (CR/LF).",
                ));
            } else {
                errors.extend(validate_url_value(
                    &query.value,
                    &format!("{field}.value"),
                    context,
                ));
            }
        }

        if let Some(template) = self.body.template() {
            errors.extend(validate_template_field(
                template,
                &format!("{path}.body.template"),
                context,
                true,
            ));
            if matches!(self.body, Body::Json { .. }) {
                if template.trim().is_empty() {
                    errors.push(FieldError::new(
                        format!("{path}.body.template"),
                        "JSON body is empty.",
                    ));
                } else {
                    let sample = substitute_samples(template, context);
                    if serde_json::from_str::<Value>(&sample).is_err() {
                        errors.push(FieldError::new(
                            format!("{path}.body.template"),
                            "JSON body is not valid after variable placeholders are substituted.",
                        ));
                    }
                }
            }
        }

        if !(1..=300_000).contains(&self.timeout_ms) {
            errors.push(FieldError::new(
                format!("{path}.timeout_ms"),
                "Timeout must be between 1 and 300,000 ms.",
            ));
        }

        match self.think_time {
            ThinkTime::None => {}
            ThinkTime::Fixed { ms } => {
                if ms > 600_000 {
                    errors.push(FieldError::new(
                        format!("{path}.think_time.ms"),
                        "Think time must be 600,000 ms or less.",
                    ));
                }
            }
            ThinkTime::Range { min_ms, max_ms } => {
                if min_ms > max_ms {
                    errors.push(FieldError::new(
                        format!("{path}.think_time.min_ms"),
                        "Minimum think time cannot exceed the maximum.",
                    ));
                }
                if max_ms > 600_000 {
                    errors.push(FieldError::new(
                        format!("{path}.think_time.max_ms"),
                        "Think time must be 600,000 ms or less.",
                    ));
                }
            }
        }

        if self.expected_status.is_empty() {
            errors.push(FieldError::new(
                format!("{path}.expected_status"),
                "Add at least one expected status code.",
            ));
        }
        let mut statuses = HashSet::new();
        for (index, status) in self.expected_status.iter().enumerate() {
            if !(100..=599).contains(status) {
                errors.push(FieldError::new(
                    format!("{path}.expected_status[{index}]"),
                    "Status codes must be between 100 and 599.",
                ));
            }
            if !statuses.insert(*status) {
                errors.push(FieldError::new(
                    format!("{path}.expected_status[{index}]"),
                    "Status codes must be unique.",
                ));
            }
        }

        for (index, assertion) in self.assertions.iter().enumerate() {
            errors.extend(assertion.validate(&format!("{path}.assertions[{index}]"), context));
        }

        let mut extraction_variables = HashSet::new();
        for (index, extraction) in self.extract.iter().enumerate() {
            let field = format!("{path}.extract[{index}]");
            let variable = extraction.variable().trim();
            if let Some(mut error) = validate_variable_key(variable) {
                error.field = format!("{field}.variable");
                errors.push(error);
            }
            if !extraction_variables.insert(variable.to_owned()) {
                errors.push(FieldError::new(
                    format!("{field}.variable"),
                    "Extraction variable names must be unique within a step.",
                ));
            }
            match extraction {
                Extraction::JsonPath { path: value, .. } => {
                    if let Err(message) = validate_json_path(value) {
                        errors.push(FieldError::new(
                            format!("{field}.expression"),
                            format!("JSON path: {message}"),
                        ));
                    }
                }
                Extraction::Regex { pattern, group, .. } => match Regex::new(pattern) {
                    Ok(regex) => {
                        if *group as usize >= regex.captures_len() {
                            errors.push(FieldError::new(
                                format!("{field}.group"),
                                format!(
                                    "Capture group {group} does not exist; this pattern has {} group(s).",
                                    regex.captures_len().saturating_sub(1)
                                ),
                            ));
                        }
                    }
                    Err(error) => errors.push(FieldError::new(
                        format!("{field}.expression"),
                        format!("Invalid regular expression: {error}"),
                    )),
                },
                Extraction::Header { name, .. } => {
                    if !is_header_name(name.trim()) {
                        errors.push(FieldError::new(
                            format!("{field}.expression"),
                            "Header name contains invalid characters.",
                        ));
                    }
                }
                Extraction::Status { .. } => {}
            }
        }

        errors
    }
}

impl Assertion {
    fn validate(&self, path: &str, context: &VariableContext) -> Vec<FieldError> {
        let mut errors = Vec::new();
        match self {
            Assertion::StatusIn { values } => {
                if values.is_empty() {
                    errors.push(FieldError::new(
                        format!("{path}.values"),
                        "Add at least one status code.",
                    ));
                }
                let mut seen = HashSet::new();
                for status in values {
                    if !(100..=599).contains(status) {
                        errors.push(FieldError::new(
                            format!("{path}.values"),
                            "Status codes must be between 100 and 599.",
                        ));
                        break;
                    }
                    if !seen.insert(*status) {
                        errors.push(FieldError::new(
                            format!("{path}.values"),
                            "Status codes must be unique.",
                        ));
                        break;
                    }
                }
            }
            Assertion::HeaderEquals { name, value } => {
                if !is_header_name(name.trim()) {
                    errors.push(FieldError::new(
                        format!("{path}.name"),
                        "Header name contains invalid characters.",
                    ));
                }
                if contains_crlf(value) {
                    errors.push(FieldError::new(
                        format!("{path}.value"),
                        "Header values cannot contain line breaks (CR/LF).",
                    ));
                } else {
                    errors.extend(validate_template_field(
                        value,
                        &format!("{path}.value"),
                        context,
                        false,
                    ));
                }
            }
            Assertion::BodyContains { value } => {
                if value.is_empty() {
                    errors.push(FieldError::new(
                        format!("{path}.value"),
                        "Text to match is required.",
                    ));
                }
                errors.extend(validate_template_field(
                    value,
                    &format!("{path}.value"),
                    context,
                    false,
                ));
            }
            Assertion::BodyRegex { pattern } => {
                if pattern.is_empty() {
                    errors.push(FieldError::new(
                        format!("{path}.pattern"),
                        "Regular expression is required.",
                    ));
                } else if let Err(error) = Regex::new(pattern) {
                    errors.push(FieldError::new(
                        format!("{path}.pattern"),
                        format!("Invalid regular expression: {error}"),
                    ));
                }
            }
            Assertion::JsonPathEquals {
                path: json_path,
                value,
            } => {
                if let Err(message) = validate_json_path(json_path) {
                    errors.push(FieldError::new(
                        format!("{path}.path"),
                        format!("JSON path: {message}"),
                    ));
                }
                if !value.is_null()
                    && !value.is_boolean()
                    && !value.is_number()
                    && !value.is_string()
                {
                    errors.push(FieldError::new(
                        format!("{path}.value"),
                        "JSON path assertions compare scalar values only.",
                    ));
                }
            }
            Assertion::MaxDurationMs { value } | Assertion::MaxSizeBytes { value } => {
                if *value == 0 {
                    errors.push(FieldError::new(
                        format!("{path}.value"),
                        "Limit must be greater than zero.",
                    ));
                }
            }
        }
        errors
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub description: String,
    pub revision: u32,
    pub variables: Vec<ScenarioVariable>,
    pub steps: Vec<Step>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl Scenario {
    pub fn new(project_id: Id, name: impl Into<String>) -> Self {
        let timestamp = now();
        Self {
            id: Id::now_v7(),
            project_id,
            name: name.into(),
            description: String::new(),
            revision: 1,
            variables: Vec::new(),
            steps: vec![Step::blank(0)],
            created_at: timestamp,
            updated_at: timestamp,
            archived_at: None,
        }
    }

    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_owned();
        self.description = self.description.trim().to_owned();
        for variable in &mut self.variables {
            variable.key = variable.key.trim().to_owned();
        }
        for step in &mut self.steps {
            step.normalize();
        }
    }

    pub fn validate(&self) -> Vec<FieldError> {
        self.validate_with_environment(None)
    }

    /// Validate with the selected environment when available. Environment
    /// context is required to reject undefined variables, secret variables in
    /// URLs/query strings, and requests that violate host pinning.
    pub fn validate_with_environment(&self, environment: Option<&Environment>) -> Vec<FieldError> {
        let mut errors = validate_scenario_name(&self.name);
        if self.description.chars().count() > 500 {
            errors.push(FieldError::new(
                "description",
                "Description must be 500 characters or fewer.",
            ));
        }

        let context = VariableContext::new(self, environment);

        let mut variable_names = HashSet::new();
        for (index, variable) in self.variables.iter().enumerate() {
            let field = format!("variables[{index}]");
            if let Some(mut error) = validate_variable_key(&variable.key) {
                error.field = format!("{field}.key");
                errors.push(error);
            }
            if !variable_names.insert(variable.key.trim().to_owned()) {
                errors.push(FieldError::new(
                    format!("{field}.key"),
                    "Scenario variable names must be unique.",
                ));
            }
        }

        if self.steps.is_empty() {
            errors.push(FieldError::new(
                "steps",
                "A scenario must contain at least one step.",
            ));
        }

        let mut step_ids = HashSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            if !step_ids.insert(step.id) {
                errors.push(FieldError::new(
                    format!("steps[{index}].id"),
                    "Step identifiers must be unique.",
                ));
            }
            errors.extend(step.validate(&format!("steps[{index}]"), &context));
        }

        errors
    }

    /// Validation for one step without requiring the rest of the scenario to
    /// be valid. Used by the single-request tester.
    pub fn validate_step(
        &self,
        index: usize,
        environment: Option<&Environment>,
    ) -> Vec<FieldError> {
        let Some(step) = self.steps.get(index) else {
            return vec![FieldError::new("steps", "Select a step to test.")];
        };
        let context = VariableContext::new(self, environment);
        step.validate(&format!("steps[{index}]"), &context)
    }
}

pub fn validate_scenario_name(name: &str) -> Vec<FieldError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return vec![FieldError::new("name", "Scenario name is required.")];
    }
    if trimmed.chars().count() > 80 {
        return vec![FieldError::new(
            "name",
            "Scenario name must be 80 characters or fewer.",
        )];
    }
    Vec::new()
}

struct VariableContext {
    known: HashSet<String>,
    secret: HashSet<String>,
    check_undefined: bool,
    environment: Option<EnvironmentContext>,
}

struct EnvironmentContext {
    base_url: String,
    base_host: Option<String>,
    allow_external_hosts: bool,
    allowed_hosts: Vec<String>,
}

impl VariableContext {
    fn new(scenario: &Scenario, environment: Option<&Environment>) -> Self {
        let mut known = HashSet::from(["base_url".to_owned(), "run_id".to_owned()]);
        let mut secret = HashSet::new();
        for variable in &scenario.variables {
            known.insert(variable.key.trim().to_owned());
        }
        // Values extracted by earlier steps are available to later steps in
        // the same iteration, so they count as statically known names.
        for step in &scenario.steps {
            for extraction in &step.extract {
                known.insert(extraction.variable().trim().to_owned());
            }
        }
        let env_context = environment.map(|environment| {
            for variable in &environment.variables {
                known.insert(variable.key.trim().to_owned());
                if variable.secret || secret_value(&variable.value) {
                    secret.insert(variable.key.trim().to_owned());
                }
            }
            EnvironmentContext {
                base_url: environment.base_url.clone(),
                base_host: environment.host(),
                allow_external_hosts: environment.allow_external_hosts,
                allowed_hosts: environment.allowed_hosts.clone(),
            }
        });
        Self {
            known,
            secret,
            check_undefined: environment.is_some(),
            environment: env_context,
        }
    }

    fn sample_for(&self, name: &str) -> String {
        if name == "base_url" {
            return self
                .environment
                .as_ref()
                .map(|environment| environment.base_url.clone())
                .unwrap_or_else(|| "http://benchlab.invalid".to_owned());
        }
        if name == "run_id" {
            return "benchlab-test-run".to_owned();
        }
        // A bare numeric placeholder is valid both inside JSON strings and in
        // scalar positions, so it is the safest sample for static JSON checks.
        "0".to_owned()
    }
}

fn secret_value(value: &Option<String>) -> bool {
    // Secret variables always have `value = None`; this helper keeps the
    // intent explicit and makes future metadata changes safer.
    value.is_none()
}

fn validate_template_field(
    template: &str,
    field: &str,
    context: &VariableContext,
    allow_secret: bool,
) -> Vec<FieldError> {
    let mut errors = Vec::new();
    match template_variables(template) {
        Ok(names) => {
            for name in names {
                if context.check_undefined && !context.known.contains(name) {
                    errors.push(FieldError::new(
                        field,
                        format!("Variable “{name}” is not defined in the scenario or environment."),
                    ));
                }
                if !allow_secret && context.secret.contains(name) {
                    errors.push(FieldError::new(
                        field,
                        format!("Secret variable “{name}” cannot be used in this field."),
                    ));
                }
            }
        }
        Err(message) => errors.push(FieldError::new(field, message)),
    }
    errors
}

fn validate_url_value(template: &str, field: &str, context: &VariableContext) -> Vec<FieldError> {
    let mut errors = Vec::new();
    match template_variables(template) {
        Ok(names) => {
            for name in names {
                if context.check_undefined && !context.known.contains(name) {
                    errors.push(FieldError::new(
                        field,
                        format!("Variable “{name}” is not defined in the scenario or environment."),
                    ));
                }
                if context.secret.contains(name) {
                    errors.push(FieldError::new(
                        field,
                        format!(
                            "Secret variable “{name}” cannot be used in URLs or query strings."
                        ),
                    ));
                }
            }
        }
        Err(message) => errors.push(FieldError::new(field, message)),
    }
    errors
}

fn validate_url_template(
    template: &str,
    field: &str,
    context: &VariableContext,
) -> Vec<FieldError> {
    let mut errors = validate_url_value(template, field, context);
    let resolved = substitute_samples(template, context);
    if contains_crlf(&resolved) {
        errors.push(FieldError::new(
            field,
            "URLs cannot contain line breaks (CR/LF).",
        ));
        return errors;
    }
    if resolved.trim().is_empty() {
        errors.push(FieldError::new(field, "Request URL is required."));
        return errors;
    }
    if resolved.starts_with("//") {
        errors.push(FieldError::new(
            field,
            "Protocol-relative URLs are not supported.",
        ));
        return errors;
    }

    let parsed = if resolved.contains("://") {
        Url::parse(&resolved)
    } else {
        Url::parse("http://benchlab.invalid/").and_then(|base| base.join(&resolved))
    };
    let Ok(url) = parsed else {
        errors.push(FieldError::new(
            field,
            "Enter a valid absolute http(s) URL or a relative path.",
        ));
        return errors;
    };
    if !matches!(url.scheme(), "http" | "https") {
        errors.push(FieldError::new(field, "URL must use http or https."));
    }
    if !url.username().is_empty() || url.password().is_some() {
        errors.push(FieldError::new(
            field,
            "URLs must not contain credentials. Put secrets in headers.",
        ));
    }
    if url.host_str().is_none() {
        errors.push(FieldError::new(field, "URL must include a host."));
    }
    if let Some(environment) = context.environment.as_ref()
        && let Some(host) = url.host_str()
        && !host_allowed(environment, host, url.port_or_known_default())
    {
        errors.push(FieldError::new(
            field,
            format!(
                "Host “{host}” is not allowed by this environment. Enable external hosts and list it, or pin the request to the base host."
            ),
        ));
    }
    errors
}

fn host_allowed(environment: &EnvironmentContext, host: &str, port: Option<u16>) -> bool {
    if environment
        .base_host
        .as_deref()
        .is_some_and(|base| base.eq_ignore_ascii_case(host))
    {
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
    })
}

fn substitute_samples(template: &str, context: &VariableContext) -> String {
    let mut output = String::with_capacity(template.len());
    let mut cursor = 0;
    while let Some(relative) = template[cursor..].find("{{") {
        let open = cursor + relative;
        output.push_str(&template[cursor..open]);
        let Some(close_relative) = template[open + 2..].find("}}") else {
            output.push_str(&template[open..]);
            return output;
        };
        let close = open + 2 + close_relative;
        let name = &template[open + 2..close];
        output.push_str(&context.sample_for(name));
        cursor = close + 2;
    }
    output.push_str(&template[cursor..]);
    output
}

fn contains_crlf(value: &str) -> bool {
    value.contains(['\r', '\n'])
}

/// Return the variable names used by a `{{name}}` template. Stray `}}` is
/// allowed because nested JSON objects naturally end with two braces.
pub fn template_variables(template: &str) -> Result<Vec<&str>, String> {
    let mut names = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = template[cursor..].find("{{") {
        let open = cursor + relative;
        let Some(close_relative) = template[open + 2..].find("}}") else {
            return Err("Unclosed variable placeholder. Use {{name}}.".to_owned());
        };
        let close = open + 2 + close_relative;
        let name = &template[open + 2..close];
        if !is_variable_name(name) {
            return Err(format!(
                "“{{{{{name}}}}}” is not a valid variable. Use letters, digits, and underscores, starting with a letter or underscore."
            ));
        }
        names.push(name);
        cursor = close + 2;
    }
    Ok(names)
}

fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

/// Replace every `{{name}}` with its value. Missing variables are errors.
pub fn interpolate(template: &str, variables: &HashMap<String, String>) -> Result<String, String> {
    let names = template_variables(template)?;
    for name in names {
        if !variables.contains_key(name) {
            return Err(format!("Variable “{name}” is not defined."));
        }
    }
    let mut output = String::with_capacity(template.len());
    let mut cursor = 0;
    while let Some(relative) = template[cursor..].find("{{") {
        let open = cursor + relative;
        output.push_str(&template[cursor..open]);
        let close = open + 2 + template[open + 2..].find("}}").expect("checked above");
        let name = &template[open + 2..close];
        output.push_str(variables.get(name).expect("checked above"));
        cursor = close + 2;
    }
    output.push_str(&template[cursor..]);
    Ok(output)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertionResult {
    pub label: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractionResult {
    pub variable: String,
    pub value: Option<String>,
    pub error: Option<String>,
}

/// Response facts used when evaluating status, header, body, duration, and size
/// assertions.
#[derive(Clone, Copy, Debug)]
pub struct ResponseFacts<'a> {
    pub status: u16,
    pub headers: &'a [(String, String)],
    pub body: &'a str,
    pub duration_ms: u64,
    pub size_bytes: u64,
}

/// Evaluate the expected status list and all configured response assertions.
pub fn evaluate_assertions(
    expected_status: &[u16],
    assertions: &[Assertion],
    response: &ResponseFacts<'_>,
    variables: &HashMap<String, String>,
) -> Vec<AssertionResult> {
    let ResponseFacts {
        status,
        headers,
        body,
        duration_ms,
        size_bytes,
    } = *response;
    let mut results = Vec::new();
    if !expected_status.is_empty() {
        let passed = expected_status.contains(&status);
        results.push(AssertionResult {
            label: format!("status in {}", join_statuses(expected_status)),
            passed,
            detail: format!("received {status}"),
        });
    }
    for assertion in assertions {
        results.push(evaluate_assertion(
            assertion,
            status,
            headers,
            body,
            duration_ms,
            size_bytes,
            variables,
        ));
    }
    results
}

fn evaluate_assertion(
    assertion: &Assertion,
    status: u16,
    headers: &[(String, String)],
    body: &str,
    duration_ms: u64,
    size_bytes: u64,
    variables: &HashMap<String, String>,
) -> AssertionResult {
    match assertion {
        Assertion::StatusIn { values } => AssertionResult {
            label: format!("status in {}", join_statuses(values)),
            passed: values.contains(&status),
            detail: format!("received {status}"),
        },
        Assertion::HeaderEquals { name, value } => {
            let expected = interpolate(value, variables).unwrap_or_else(|_| value.clone());
            match header_value(headers, name) {
                Some(actual) if actual == expected => AssertionResult {
                    label: format!("header {name} equals expected value"),
                    passed: true,
                    detail: format!("{name}: {actual}"),
                },
                Some(actual) => AssertionResult {
                    label: format!("header {name} equals expected value"),
                    passed: false,
                    detail: format!("received {name}: {actual}"),
                },
                None => AssertionResult {
                    label: format!("header {name} equals expected value"),
                    passed: false,
                    detail: format!("header {name} was not present"),
                },
            }
        }
        Assertion::BodyContains { value } => {
            let expected = interpolate(value, variables).unwrap_or_else(|_| value.clone());
            let passed = body.contains(&expected);
            AssertionResult {
                label: "body contains expected text".to_owned(),
                passed,
                detail: if passed {
                    "expected text was found".to_owned()
                } else {
                    "expected text was not found".to_owned()
                },
            }
        }
        Assertion::BodyRegex { pattern } => match Regex::new(pattern) {
            Ok(regex) => {
                let passed = regex.is_match(body);
                AssertionResult {
                    label: format!("body matches /{pattern}/"),
                    passed,
                    detail: if passed {
                        "pattern matched".to_owned()
                    } else {
                        "pattern did not match".to_owned()
                    },
                }
            }
            Err(error) => AssertionResult {
                label: format!("body matches /{pattern}/"),
                passed: false,
                detail: format!("invalid pattern: {error}"),
            },
        },
        Assertion::JsonPathEquals {
            path,
            value: expected,
        } => {
            let parsed = serde_json::from_str::<Value>(body);
            match parsed {
                Ok(json) => match json_path_get(&json, path) {
                    Ok(Some(actual)) if actual == expected => AssertionResult {
                        label: format!("{path} equals {}", json_scalar_text(expected)),
                        passed: true,
                        detail: format!("received {}", json_scalar_text(actual)),
                    },
                    Ok(Some(actual)) => AssertionResult {
                        label: format!("{path} equals {}", json_scalar_text(expected)),
                        passed: false,
                        detail: format!("received {}", json_scalar_text(actual)),
                    },
                    Ok(None) => AssertionResult {
                        label: format!("{path} equals {}", json_scalar_text(expected)),
                        passed: false,
                        detail: "path did not resolve".to_owned(),
                    },
                    Err(message) => AssertionResult {
                        label: format!("{path} equals {}", json_scalar_text(expected)),
                        passed: false,
                        detail: message,
                    },
                },
                Err(_) => AssertionResult {
                    label: format!("{path} equals {}", json_scalar_text(expected)),
                    passed: false,
                    detail: "response body is not valid JSON".to_owned(),
                },
            }
        }
        Assertion::MaxDurationMs { value } => AssertionResult {
            label: format!("duration ≤ {value} ms"),
            passed: duration_ms <= *value,
            detail: format!("duration was {duration_ms} ms"),
        },
        Assertion::MaxSizeBytes { value } => AssertionResult {
            label: format!("size ≤ {value} bytes"),
            passed: size_bytes <= *value,
            detail: format!("size was {size_bytes} bytes"),
        },
    }
}

/// Evaluate extraction rules against a response. Each rule yields either a
/// value or an explanation; one failure does not hide the other results.
pub fn evaluate_extractions(
    extractions: &[Extraction],
    status: u16,
    headers: &[(String, String)],
    body: &str,
) -> Vec<ExtractionResult> {
    let parsed_json = serde_json::from_str::<Value>(body).ok();
    extractions
        .iter()
        .map(|extraction| {
            let variable = extraction.variable().to_owned();
            let result = match extraction {
                Extraction::JsonPath { path, .. } => match parsed_json.as_ref() {
                    Some(json) => match json_path_get(json, path) {
                        Ok(Some(value)) if is_scalar(value) => Ok(json_scalar_text(value)),
                        Ok(Some(_)) => Err("JSON path resolved to an object or array.".to_owned()),
                        Ok(None) => Err("JSON path did not resolve.".to_owned()),
                        Err(message) => Err(message),
                    },
                    None => Err("response body is not valid JSON".to_owned()),
                },
                Extraction::Regex { pattern, group, .. } => match Regex::new(pattern) {
                    Ok(regex) => match regex.captures(body) {
                        Some(captures) => match captures.get(*group as usize) {
                            Some(capture) => Ok(capture.as_str().to_owned()),
                            None => Err(format!("capture group {group} did not match")),
                        },
                        None => Err("pattern did not match the response body".to_owned()),
                    },
                    Err(error) => Err(format!("invalid regular expression: {error}")),
                },
                Extraction::Header { name, .. } => header_value(headers, name)
                    .map(str::to_owned)
                    .ok_or_else(|| format!("header “{name}” was not present")),
                Extraction::Status { .. } => Ok(status.to_string()),
            };
            match result {
                Ok(value) => ExtractionResult {
                    variable,
                    value: Some(value),
                    error: None,
                },
                Err(error) => ExtractionResult {
                    variable,
                    value: None,
                    error: Some(error),
                },
            }
        })
        .collect()
}

fn header_value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn join_statuses(statuses: &[u16]) -> String {
    statuses
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_scalar(value: &Value) -> bool {
    value.is_null() || value.is_boolean() || value.is_number() || value.is_string()
}

pub fn json_scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        other => other.to_string(),
    }
}

/// Parse a UI scalar input. Valid JSON scalar literals keep their type; every
/// other input becomes a string. Quote a number to compare it as text.
pub fn parse_scalar_input(input: &str) -> Value {
    if let Ok(value) = serde_json::from_str::<Value>(input.trim())
        && is_scalar(&value)
    {
        return value;
    }
    Value::String(input.to_owned())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JsonPathToken {
    Key(String),
    Index(usize),
}

/// Validate a scalar JSONPath expression. Supported: `$`, `.key`, `['key']`,
/// `["key"]`, and `[index]`. Wildcards and filters are intentionally rejected.
pub fn validate_json_path(path: &str) -> Result<(), String> {
    json_path_tokens(path).map(|_| ())
}

fn json_path_tokens(path: &str) -> Result<Vec<JsonPathToken>, String> {
    let trimmed = path.trim();
    if !trimmed.starts_with('$') {
        return Err("path must start with $".to_owned());
    }
    let mut tokens = Vec::new();
    let mut cursor = 1;
    while cursor < trimmed.len() {
        let ch = trimmed[cursor..]
            .chars()
            .next()
            .expect("cursor is in range");
        if ch == '.' {
            cursor += 1;
            let start = cursor;
            while cursor < trimmed.len() {
                let ch = trimmed[cursor..]
                    .chars()
                    .next()
                    .expect("cursor is in range");
                if ch == '.' || ch == '[' {
                    break;
                }
                cursor += ch.len_utf8();
            }
            if start == cursor {
                return Err("empty property name after '.'".to_owned());
            }
            tokens.push(JsonPathToken::Key(trimmed[start..cursor].to_owned()));
        } else if ch == '[' {
            let close = trimmed[cursor..]
                .find(']')
                .map(|index| cursor + index)
                .ok_or_else(|| "unclosed '['".to_owned())?;
            let inner = trimmed[cursor + 1..close].trim();
            if inner.is_empty() {
                return Err("empty brackets".to_owned());
            }
            if (inner.starts_with('\'') && inner.ends_with('\''))
                || (inner.starts_with('"') && inner.ends_with('"'))
            {
                if inner.len() < 2 {
                    return Err("empty quoted key".to_owned());
                }
                let key = &inner[1..inner.len() - 1];
                if key.is_empty() {
                    return Err("empty quoted key".to_owned());
                }
                tokens.push(JsonPathToken::Key(key.to_owned()));
            } else {
                let index = inner
                    .parse::<usize>()
                    .map_err(|_| "index must be a non-negative integer".to_owned())?;
                tokens.push(JsonPathToken::Index(index));
            }
            cursor = close + 1;
        } else {
            return Err(format!("unexpected character “{ch}”"));
        }
    }
    Ok(tokens)
}

fn json_path_get<'a>(root: &'a Value, path: &str) -> Result<Option<&'a Value>, String> {
    let tokens = json_path_tokens(path)?;
    let mut current = root;
    for token in tokens {
        current = match token {
            JsonPathToken::Key(key) => match current {
                Value::Object(object) => match object.get(&key) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                _ => return Ok(None),
            },
            JsonPathToken::Index(index) => match current {
                Value::Array(array) => match array.get(index) {
                    Some(value) => value,
                    None => return Ok(None),
                },
                _ => return Ok(None),
            },
        };
    }
    Ok(Some(current))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EnvironmentKind, Variable};
    use std::collections::HashMap;

    fn scenario_with_step() -> Scenario {
        Scenario::new(Id::now_v7(), "Inventory")
    }

    fn environment() -> Environment {
        let mut environment = Environment::new(
            Id::now_v7(),
            "Local",
            EnvironmentKind::Local,
            "http://127.0.0.1:8080",
        )
        .unwrap();
        environment.variables.push(Variable::plain("token", "abc"));
        environment
            .variables
            .push(Variable::secret("api_key", Id::now_v7()));
        environment
    }

    #[test]
    fn blank_scenario_starts_with_one_valid_step() {
        let scenario = scenario_with_step();
        assert_eq!(scenario.steps.len(), 1);
        assert!(scenario.validate().is_empty());
    }

    #[test]
    fn templates_and_reserved_names_are_validated() {
        assert_eq!(
            template_variables("{{base_url}}/items/{{id}}")
                .unwrap()
                .len(),
            2
        );
        assert!(template_variables("{{bad name}}").is_err());
        assert!(template_variables("{{unclosed").is_err());
        assert!(template_variables(r#"{"a":{"b":1}}"#).unwrap().is_empty());

        let mut scenario = scenario_with_step();
        scenario
            .variables
            .push(ScenarioVariable::new("base_url", "nope"));
        let errors = scenario.validate();
        assert!(errors.iter().any(|error| error.field == "variables[0].key"));
    }

    #[test]
    fn secret_variables_are_rejected_in_urls_and_queries() {
        let environment = environment();
        let mut scenario = scenario_with_step();
        scenario.steps[0].url = "{{base_url}}/items?key={{api_key}}".to_owned();
        let errors = scenario.validate_with_environment(Some(&environment));
        assert!(
            errors
                .iter()
                .any(|error| error.field == "steps[0].url" && error.message.contains("Secret"))
        );

        scenario.steps[0].url = "{{base_url}}/items".to_owned();
        scenario.steps[0]
            .query
            .push(KeyValue::new("key", "{{api_key}}"));
        let errors = scenario.validate_with_environment(Some(&environment));
        assert!(
            errors
                .iter()
                .any(|error| error.field == "steps[0].query[0].value")
        );
    }

    #[test]
    fn host_pinning_is_validated() {
        let environment = environment();
        let mut scenario = scenario_with_step();
        scenario.steps[0].url = "https://example.com/items".to_owned();
        let errors = scenario.validate_with_environment(Some(&environment));
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("not allowed"))
        );
    }

    #[test]
    fn headers_reject_crlf_and_duplicates() {
        let mut scenario = scenario_with_step();
        scenario.steps[0].headers = vec![
            KeyValue::new("X-Test", "a\r\nb"),
            KeyValue::new("X-Test", "c"),
        ];
        let errors = scenario.validate();
        assert!(errors.iter().any(|error| error.message.contains("CR/LF")));
        assert!(errors.iter().any(|error| error.message.contains("unique")));
    }

    #[test]
    fn json_path_supports_fields_and_indices() {
        let value: Value = serde_json::from_str(r#"{"items":[{"id":7,"name":"bolt"}]}"#).unwrap();
        assert_eq!(
            json_path_get(&value, "$.items[0].id").unwrap(),
            Some(&Value::from(7))
        );
        assert_eq!(
            json_path_get(&value, "$['items'][0]['name']").unwrap(),
            Some(&Value::from("bolt"))
        );
        assert!(json_path_get(&value, "$.items[*].id").is_err());
        assert!(validate_json_path("$..id").is_err());
    }

    #[test]
    fn assertions_and_extractions_evaluate_against_a_response() {
        let body = r#"{"items":[{"id":7,"name":"bolt"}],"ok":true}"#;
        let assertions = vec![
            Assertion::HeaderEquals {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            },
            Assertion::BodyContains {
                value: "\"ok\":true".to_owned(),
            },
            Assertion::BodyRegex {
                pattern: r#""id":7"#.to_owned(),
            },
            Assertion::JsonPathEquals {
                path: "$.items[0].name".to_owned(),
                value: Value::from("bolt"),
            },
            Assertion::MaxDurationMs { value: 100 },
            Assertion::MaxSizeBytes { value: 500 },
        ];
        let headers = vec![("content-type".to_owned(), "application/json".to_owned())];
        let response = ResponseFacts {
            status: 200,
            headers: &headers,
            body,
            duration_ms: 25,
            size_bytes: 100,
        };
        let results = evaluate_assertions(&[200], &assertions, &response, &HashMap::new());
        assert!(results.iter().all(|result| result.passed), "{results:?}");

        let extractions = vec![
            Extraction::JsonPath {
                variable: "item_id".to_owned(),
                path: "$.items[0].id".to_owned(),
            },
            Extraction::Regex {
                variable: "word".to_owned(),
                pattern: "\"name\":\"(\\w+)\"".to_owned(),
                group: 1,
            },
            Extraction::Header {
                variable: "ctype".to_owned(),
                name: "Content-Type".to_owned(),
            },
            Extraction::Status {
                variable: "status".to_owned(),
            },
        ];
        let results = evaluate_extractions(&extractions, 200, &headers, body);
        assert_eq!(results[0].value.as_deref(), Some("7"));
        assert_eq!(results[1].value.as_deref(), Some("bolt"));
        assert_eq!(results[2].value.as_deref(), Some("application/json"));
        assert_eq!(results[3].value.as_deref(), Some("200"));
    }

    #[test]
    fn interpolation_reports_missing_variables() {
        let variables = HashMap::from([("base_url".to_owned(), "http://localhost".to_owned())]);
        assert_eq!(
            interpolate("{{base_url}}/health", &variables).unwrap(),
            "http://localhost/health"
        );
        assert!(interpolate("{{missing}}", &variables).is_err());
    }

    #[test]
    fn scalar_input_keeps_json_types() {
        assert_eq!(parse_scalar_input("42"), Value::from(42));
        assert_eq!(parse_scalar_input("true"), Value::from(true));
        assert_eq!(parse_scalar_input("\"42\""), Value::from("42"));
        assert_eq!(parse_scalar_input("bolt"), Value::from("bolt"));
    }
}
