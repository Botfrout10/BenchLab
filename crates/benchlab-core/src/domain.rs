//! Core entities, enums, and validation shared by the store and UI.

use std::{collections::HashSet, fmt};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::Id;

/// Return the current UTC timestamp used for all persisted timestamps.
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// The fixed project accent palette from SPEC.md §8.2.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Blue,
    Teal,
    Violet,
    Amber,
    Rose,
}

impl Accent {
    pub const ALL: [Accent; 5] = [
        Accent::Blue,
        Accent::Teal,
        Accent::Violet,
        Accent::Amber,
        Accent::Rose,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Accent::Blue => "blue",
            Accent::Teal => "teal",
            Accent::Violet => "violet",
            Accent::Amber => "amber",
            Accent::Rose => "rose",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Accent::Blue => "Blue",
            Accent::Teal => "Teal",
            Accent::Violet => "Violet",
            Accent::Amber => "Amber",
            Accent::Rose => "Rose",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|accent| accent.as_str() == value)
    }
}

impl fmt::Display for Accent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A workspace project. Archived projects are hidden from the default list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: Id,
    pub name: String,
    pub description: String,
    pub accent: Accent,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

impl Project {
    pub fn new(name: &str, description: &str, accent: Accent) -> Result<Self, Vec<FieldError>> {
        let timestamp = now();
        let project = Self {
            id: Id::now_v7(),
            name: name.trim().to_owned(),
            description: description.trim().to_owned(),
            accent,
            created_at: timestamp,
            updated_at: timestamp,
            archived_at: None,
        };
        let errors = validate_project(&project.name, &project.description);
        if errors.is_empty() {
            Ok(project)
        } else {
            Err(errors)
        }
    }

    pub fn validate(&self) -> Vec<FieldError> {
        validate_project(&self.name, &self.description)
    }
}

/// Environment class. The class controls conservative default safety caps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentKind {
    #[default]
    Local,
    Staging,
    Production,
}

impl EnvironmentKind {
    pub const ALL: [EnvironmentKind; 3] = [
        EnvironmentKind::Local,
        EnvironmentKind::Staging,
        EnvironmentKind::Production,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EnvironmentKind::Local => "local",
            EnvironmentKind::Staging => "staging",
            EnvironmentKind::Production => "production",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EnvironmentKind::Local => "Local",
            EnvironmentKind::Staging => "Staging",
            EnvironmentKind::Production => "Production",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// Safe starting caps from SPEC.md §19.4. Users can edit them.
    pub fn default_caps(self) -> SafetyCaps {
        match self {
            EnvironmentKind::Local => SafetyCaps {
                max_vus: 500,
                max_rps: None,
                max_duration_s: 3600,
                require_confirmation: false,
            },
            EnvironmentKind::Staging => SafetyCaps {
                max_vus: 100,
                max_rps: Some(500),
                max_duration_s: 900,
                require_confirmation: true,
            },
            EnvironmentKind::Production => SafetyCaps {
                max_vus: 20,
                max_rps: Some(100),
                max_duration_s: 300,
                require_confirmation: true,
            },
        }
    }
}

impl fmt::Display for EnvironmentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyCaps {
    pub max_vus: u32,
    pub max_rps: Option<u32>,
    pub max_duration_s: u32,
    pub require_confirmation: bool,
}

impl Default for SafetyCaps {
    fn default() -> Self {
        EnvironmentKind::Local.default_caps()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub key: String,
    /// Always `None` for secrets. Non-secret values are stored inline.
    pub value: Option<String>,
    pub secret: bool,
    /// Metadata row in SQLite. The value lives in the OS keychain.
    pub secret_ref: Option<Id>,
}

impl Variable {
    pub fn plain(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: Some(value.into()),
            secret: false,
            secret_ref: None,
        }
    }

    pub fn secret(key: impl Into<String>, secret_ref: Id) -> Self {
        Self {
            key: key.into(),
            value: None,
            secret: true,
            secret_ref: Some(secret_ref),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuthConfig {
    #[default]
    None,
    Bearer {
        token_var: String,
    },
    Basic {
        username_var: String,
        password_var: String,
    },
    Header {
        name: String,
        value_var: String,
    },
}

impl AuthConfig {
    pub fn label(&self) -> &'static str {
        match self {
            AuthConfig::None => "None",
            AuthConfig::Bearer { .. } => "Bearer token",
            AuthConfig::Basic { .. } => "Basic auth",
            AuthConfig::Header { .. } => "Custom header",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub id: Id,
    pub project_id: Id,
    pub name: String,
    pub kind: EnvironmentKind,
    pub base_url: String,
    pub variables: Vec<Variable>,
    pub auth: AuthConfig,
    pub tls_verify: bool,
    pub allow_external_hosts: bool,
    pub allowed_hosts: Vec<String>,
    pub caps: SafetyCaps,
    pub revision: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Environment archiving is required by FR-ENV-01. The initial schema
    /// omitted this column; migration 0001 includes it explicitly.
    #[serde(default)]
    pub archived_at: Option<DateTime<Utc>>,
}

impl Environment {
    pub fn new(
        project_id: Id,
        name: &str,
        kind: EnvironmentKind,
        base_url: &str,
    ) -> Result<Self, Vec<FieldError>> {
        let timestamp = now();
        let environment = Self {
            id: Id::now_v7(),
            project_id,
            name: name.trim().to_owned(),
            kind,
            base_url: base_url.trim().to_owned(),
            variables: Vec::new(),
            auth: AuthConfig::None,
            tls_verify: true,
            allow_external_hosts: false,
            allowed_hosts: Vec::new(),
            caps: kind.default_caps(),
            revision: 1,
            created_at: timestamp,
            updated_at: timestamp,
            archived_at: None,
        };
        let errors = environment.validate();
        if errors.is_empty() {
            Ok(environment)
        } else {
            Err(errors)
        }
    }

    pub fn validate(&self) -> Vec<FieldError> {
        let mut errors = validate_environment_name(&self.name);
        errors.extend(validate_base_url(&self.base_url));
        errors.extend(validate_variables(&self.variables));
        errors.extend(validate_auth(&self.auth));
        errors.extend(validate_caps(&self.caps));
        errors.extend(validate_allowed_hosts(
            self.allow_external_hosts,
            &self.allowed_hosts,
        ));
        errors
    }

    /// Trim user-editable text before persisting.
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_owned();
        self.base_url = self.base_url.trim().to_owned();
        self.allowed_hosts = self
            .allowed_hosts
            .iter()
            .map(|host| host.trim().to_owned())
            .filter(|host| !host.is_empty())
            .collect();
        for variable in &mut self.variables {
            variable.key = variable.key.trim().to_owned();
            if let Some(value) = &mut variable.value {
                // Variable values are user data, but leading/trailing whitespace
                // is almost always accidental and breaks header values.
                *value = value.trim().to_owned();
            }
        }
    }

    pub fn host(&self) -> Option<String> {
        Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    /// Dot/bracket path such as `variables[2].key`.
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

pub fn validate_project(name: &str, _description: &str) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let trimmed = name.trim();
    if trimmed.is_empty() {
        errors.push(FieldError::new("name", "Project name is required."));
    } else if trimmed.chars().count() > 80 {
        errors.push(FieldError::new(
            "name",
            "Project name must be 80 characters or fewer.",
        ));
    }
    errors
}

pub fn validate_environment_name(name: &str) -> Vec<FieldError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return vec![FieldError::new("name", "Environment name is required.")];
    }
    if trimmed.chars().count() > 60 {
        return vec![FieldError::new(
            "name",
            "Environment name must be 60 characters or fewer.",
        )];
    }
    Vec::new()
}

pub fn validate_base_url(value: &str) -> Vec<FieldError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return vec![FieldError::new("base_url", "Base URL is required.")];
    }
    let parsed = match Url::parse(trimmed) {
        Ok(url) => url,
        Err(_) => {
            return vec![FieldError::new(
                "base_url",
                "Enter an absolute http or https URL.",
            )];
        }
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return vec![FieldError::new(
            "base_url",
            "Base URL must start with http:// or https://.",
        )];
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return vec![FieldError::new(
            "base_url",
            "Base URL must not contain a username or password.",
        )];
    }
    if parsed.host_str().is_none() {
        return vec![FieldError::new(
            "base_url",
            "Base URL must include a valid host.",
        )];
    }
    Vec::new()
}

pub fn validate_variable_key(key: &str) -> Option<FieldError> {
    let key = key.trim();
    if key.is_empty() {
        return Some(FieldError::new("key", "Variable name is required."));
    }
    if !is_valid_key(key) {
        return Some(FieldError::new(
            "key",
            "Use a name like token or API_KEY (letters, digits, underscore).",
        ));
    }
    if key == "base_url" || key == "run_id" || key.starts_with("__") {
        return Some(FieldError::new("key", "This name is reserved by BenchLab."));
    }
    None
}

fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

pub fn validate_variables(variables: &[Variable]) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    for (index, variable) in variables.iter().enumerate() {
        let path = format!("variables[{index}]");
        if let Some(mut error) = validate_variable_key(&variable.key) {
            error.field = format!("{path}.key");
            errors.push(error);
        }
        if !seen.insert(variable.key.trim().to_owned()) {
            errors.push(FieldError::new(
                format!("{path}.key"),
                "Variable names must be unique.",
            ));
        }
        if variable.secret {
            if variable.value.is_some() {
                errors.push(FieldError::new(
                    format!("{path}.value"),
                    "Secret values are stored in the OS keychain, not inline.",
                ));
            }
            if variable.secret_ref.is_none() {
                errors.push(FieldError::new(
                    format!("{path}.value"),
                    "Secret value is missing.",
                ));
            }
        } else if variable.value.is_none() {
            errors.push(FieldError::new(
                format!("{path}.value"),
                "Enter a value or mark this variable as secret.",
            ));
        }
    }
    errors
}

pub fn validate_auth(auth: &AuthConfig) -> Vec<FieldError> {
    let mut errors = Vec::new();
    let check_var = |errors: &mut Vec<FieldError>, field: &str, value: &str| {
        if let Some(mut error) = validate_variable_key(value) {
            error.field = field.to_owned();
            errors.push(error);
        }
    };
    match auth {
        AuthConfig::None => {}
        AuthConfig::Bearer { token_var } => check_var(&mut errors, "auth.token_var", token_var),
        AuthConfig::Basic {
            username_var,
            password_var,
        } => {
            check_var(&mut errors, "auth.username_var", username_var);
            check_var(&mut errors, "auth.password_var", password_var);
        }
        AuthConfig::Header { name, value_var } => {
            if !is_header_name(name) {
                errors.push(FieldError::new(
                    "auth.name",
                    "Header name contains invalid characters.",
                ));
            }
            check_var(&mut errors, "auth.value_var", value_var);
        }
    }
    errors
}

pub fn is_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub fn validate_caps(caps: &SafetyCaps) -> Vec<FieldError> {
    let mut errors = Vec::new();
    if !(1..=10_000).contains(&caps.max_vus) {
        errors.push(FieldError::new(
            "caps.max_vus",
            "Max VUs must be between 1 and 10,000.",
        ));
    }
    if let Some(max_rps) = caps.max_rps
        && !(1..=100_000).contains(&max_rps)
    {
        errors.push(FieldError::new(
            "caps.max_rps",
            "Max RPS must be between 1 and 100,000, or left empty.",
        ));
    }
    if !(10..=86_400).contains(&caps.max_duration_s) {
        errors.push(FieldError::new(
            "caps.max_duration_s",
            "Max duration must be between 10 and 86,400 seconds.",
        ));
    }
    errors
}

pub fn validate_allowed_hosts(enabled: bool, hosts: &[String]) -> Vec<FieldError> {
    if !enabled {
        return Vec::new();
    }
    if hosts.iter().all(|host| host.trim().is_empty()) {
        return vec![FieldError::new(
            "allowed_hosts",
            "Add at least one external host, or turn host pinning back on.",
        )];
    }
    hosts
        .iter()
        .enumerate()
        .filter_map(|(index, host)| {
            let trimmed = host.trim();
            if trimmed.is_empty() {
                return None;
            }
            let parsed = Url::parse(&format!("http://{trimmed}")).ok();
            let valid = parsed.is_some_and(|url| {
                url.host_str().is_some()
                    && url.path() == "/"
                    && url.query().is_none()
                    && url.username().is_empty()
            });
            (!valid).then(|| {
                FieldError::new(
                    format!("allowed_hosts[{index}]"),
                    "Use a host and optional port, for example api.example.com:443.",
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_names_are_trimmed_and_bounded() {
        assert!(Project::new("  Inventory API  ", "", Accent::Blue).is_ok());
        assert!(Project::new("", "", Accent::Blue).is_err());
        assert!(Project::new(&"x".repeat(81), "", Accent::Blue).is_err());
    }

    #[test]
    fn base_urls_require_http_and_no_credentials() {
        assert!(validate_base_url("http://localhost:3000").is_empty());
        assert!(validate_base_url("https://api.example.com/v1").is_empty());
        assert!(!validate_base_url("api.example.com").is_empty());
        assert!(!validate_base_url("ftp://example.com").is_empty());
        assert!(!validate_base_url("https://user:pass@example.com").is_empty());
    }

    #[test]
    fn secret_variables_never_carry_inline_values() {
        let inline_secret = Variable {
            key: "token".into(),
            value: Some("leak".into()),
            secret: true,
            secret_ref: Some(Id::now_v7()),
        };
        assert!(!validate_variables(&[inline_secret]).is_empty());

        let secret = Variable::secret("token", Id::now_v7());
        assert!(validate_variables(&[secret]).is_empty());
    }

    #[test]
    fn caps_are_bounded() {
        assert!(validate_caps(&SafetyCaps::default()).is_empty());
        assert!(
            !validate_caps(&SafetyCaps {
                max_vus: 0,
                ..Default::default()
            })
            .is_empty()
        );
        assert!(
            !validate_caps(&SafetyCaps {
                max_duration_s: 5,
                ..Default::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn host_pinning_requires_hosts_when_disabled() {
        assert!(!validate_allowed_hosts(true, &[]).is_empty());
        assert!(validate_allowed_hosts(true, &["api.example.com".into()]).is_empty());
        assert!(!validate_allowed_hosts(true, &["https://api.example.com/path".into()]).is_empty());
    }

    #[test]
    fn environment_defaults_are_conservative() {
        assert_eq!(EnvironmentKind::Production.default_caps().max_vus, 20);
        assert_eq!(
            EnvironmentKind::Production.default_caps().max_rps,
            Some(100)
        );
        assert!(EnvironmentKind::Staging.default_caps().require_confirmation);
    }
}
