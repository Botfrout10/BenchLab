//! The application shell: top bar, workspace navigation, welcome screen,
//! project overview, environment management, scenario library, modals, and status bar.
//!
//! Views depend only on `BenchLabBackend` and the `AppView` view model defined
//! in `crate::backend`; they never touch SQLite, the keychain, or the engine.

use std::{future::Future, sync::Arc, time::Duration};

use benchlab_core::{
    Accent, AuthConfig, Environment, EnvironmentKind, FieldError, Id, Project, SafetyCaps,
    Scenario, Variable, now,
};
use chrono::{DateTime, Local, Utc};
use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, FontWeight, MouseButton,
    MouseDownEvent, Render, SharedString, Stateful, WeakEntity, Window, actions, div, prelude::*,
    px,
};

use crate::{
    backend::{AppView, BackendError, BackendResult, BenchLabBackend, SecretStorage, SecretUpdate},
    components::{
        BannerKind, ButtonSize, ButtonVariant, Column, Select, TextInput, ToastKind, banner,
        button, checkbox, disabled, empty_state, environment_badge, field, field_label,
        inline_message, modal_body, modal_footer, modal_header, modal_panel, neutral_badge,
        row_action, success_badge, switch, table_header, toast_card, warning_badge,
    },
    scenario_editor::ScenarioEditor,
    theme,
};

actions!(shell, [CloseModal]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Welcome,
    Overview,
    Scenarios,
}

pub struct Shell {
    backend: Arc<dyn BenchLabBackend>,
    app: AppView,
    focus_handle: FocusHandle,
    search: Entity<TextInput>,
    env_select: Entity<Select>,
    route: Route,
    projects: Vec<Project>,
    current_project: Option<Project>,
    environments: Vec<Environment>,
    selected_environment: Option<Id>,
    scenarios: Vec<Scenario>,
    scenario_editor: Option<Entity<ScenarioEditor>>,
    pending_scenario: Option<Scenario>,
    show_archived_projects: bool,
    show_archived_envs: bool,
    show_archived_scenarios: bool,
    loading: bool,
    load_error: Option<SharedString>,
    modal: Option<Modal>,
    toasts: Vec<ToastEntry>,
    next_toast_id: u64,
}

#[derive(Clone)]
struct ToastEntry {
    id: u64,
    kind: ToastKind,
    message: SharedString,
}

enum Modal {
    Project(ProjectModal),
    DeleteProject(DeleteProjectModal),
    ArchiveProject(ProjectRefModal),
    SwitchProject(ProjectSwitcher),
    Environment(Box<EnvironmentModal>),
    DeleteEnvironment(EnvironmentRefModal),
    ArchiveEnvironment(EnvironmentRefModal),
    DeleteScenario(ScenarioRefModal),
    ArchiveScenario(ScenarioRefModal),
    Settings,
}

impl Modal {
    fn dismissible(&self) -> bool {
        !matches!(
            self,
            Modal::DeleteProject(_)
                | Modal::DeleteEnvironment(_)
                | Modal::ArchiveProject(_)
                | Modal::ArchiveEnvironment(_)
                | Modal::DeleteScenario(_)
                | Modal::ArchiveScenario(_)
        )
    }
}

struct ProjectModal {
    existing: Option<Project>,
    name: Entity<TextInput>,
    description: Entity<TextInput>,
    accent: Accent,
    error: Option<SharedString>,
    saving: bool,
}

struct DeleteProjectModal {
    project: Project,
    confirm: Entity<TextInput>,
    error: Option<SharedString>,
}

struct ProjectRefModal {
    project: Project,
}

struct ProjectSwitcher {
    filter: Entity<TextInput>,
}

struct EnvironmentRefModal {
    environment: Environment,
}

struct ScenarioRefModal {
    scenario: Scenario,
}

struct EnvironmentModal {
    existing: Option<Environment>,
    project_id: Id,
    name: Entity<TextInput>,
    base_url: Entity<TextInput>,
    kind: EnvironmentKind,
    auth_mode: AuthMode,
    auth_token_var: Entity<TextInput>,
    auth_username_var: Entity<TextInput>,
    auth_password_var: Entity<TextInput>,
    auth_header_name: Entity<TextInput>,
    auth_header_value_var: Entity<TextInput>,
    max_vus: Entity<TextInput>,
    max_rps: Entity<TextInput>,
    max_duration: Entity<TextInput>,
    require_confirmation: bool,
    tls_verify: bool,
    allow_external_hosts: bool,
    allowed_hosts: Entity<TextInput>,
    variables: Vec<VariableRow>,
    error: Option<SharedString>,
    saving: bool,
}

struct VariableRow {
    key: Entity<TextInput>,
    value: Entity<TextInput>,
    secret: bool,
    secret_ref: Option<Id>,
    existing_secret: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AuthMode {
    None,
    Bearer,
    Basic,
    Header,
}

impl AuthMode {
    const ALL: [AuthMode; 4] = [
        AuthMode::None,
        AuthMode::Bearer,
        AuthMode::Basic,
        AuthMode::Header,
    ];

    fn label(self) -> &'static str {
        match self {
            AuthMode::None => "None",
            AuthMode::Bearer => "Bearer token",
            AuthMode::Basic => "Basic auth",
            AuthMode::Header => "Custom header",
        }
    }

    fn from_auth(auth: &AuthConfig) -> Self {
        match auth {
            AuthConfig::None => AuthMode::None,
            AuthConfig::Bearer { .. } => AuthMode::Bearer,
            AuthConfig::Basic { .. } => AuthMode::Basic,
            AuthConfig::Header { .. } => AuthMode::Header,
        }
    }
}

impl Shell {
    pub fn new(backend: Arc<dyn BenchLabBackend>, app: AppView, cx: &mut Context<Self>) -> Self {
        let search = text_input(cx, "Search projects…", "");
        let weak = cx.entity().downgrade();
        search.update(cx, |input, _| {
            input.set_on_change(Some(Box::new(move |_, _window, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.route = Route::Welcome;
                    cx.notify();
                });
            })));
        });
        let env_select = cx.new(|cx| Select::new(Vec::<SharedString>::new(), 0, cx));

        let mut shell = Self {
            backend,
            app,
            focus_handle: cx.focus_handle(),
            search,
            env_select,
            route: Route::Welcome,
            projects: Vec::new(),
            current_project: None,
            environments: Vec::new(),
            selected_environment: None,
            scenarios: Vec::new(),
            scenario_editor: None,
            pending_scenario: None,
            show_archived_projects: false,
            show_archived_envs: false,
            show_archived_scenarios: false,
            loading: true,
            load_error: None,
            modal: None,
            toasts: Vec::new(),
            next_toast_id: 1,
        };
        shell.load_initial(cx);
        shell
    }

    pub fn focus_search(&self, window: &mut Window, cx: &App) {
        window.focus(&self.search.read(cx).focus_handle(cx));
    }

    fn load_initial(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        self.load_error = None;
        let backend = self.backend.clone();
        self.run_backend(
            cx,
            async move {
                let projects = backend.list_projects(false).await?;
                let last_project = backend.last_project_id().await?;
                let current = last_project
                    .and_then(|id| projects.iter().find(|project| project.id == id).cloned());
                let environments = match &current {
                    Some(project) => backend.list_environments(project.id, false).await?,
                    None => Vec::new(),
                };
                let scenarios = match &current {
                    Some(project) => backend.list_scenarios(project.id, false).await?,
                    None => Vec::new(),
                };
                Ok((projects, current, last_project, environments, scenarios))
            },
            |this, result, cx| match result {
                Ok((projects, current, last_project, environments, scenarios)) => {
                    this.loading = false;
                    this.projects = projects;
                    this.current_project = current;
                    this.environments = environments;
                    this.scenarios = scenarios;
                    this.route = if this.current_project.is_some() {
                        Route::Overview
                    } else {
                        Route::Welcome
                    };
                    this.selected_environment = this.environments.first().map(|env| env.id);
                    this.sync_env_select(cx);
                    if last_project.is_some() && this.current_project.is_none() {
                        this.persist_last_project(None, cx);
                    }
                }
                Err(error) => {
                    this.loading = false;
                    this.load_error = Some(error.message.into());
                }
            },
        );
    }

    fn run_backend<T, F>(
        &mut self,
        cx: &mut Context<Self>,
        future: F,
        done: impl FnOnce(&mut Self, BackendResult<T>, &mut Context<Self>) + 'static,
    ) where
        T: Send + 'static,
        F: Future<Output = BackendResult<T>> + Send + 'static,
    {
        let task = cx.background_executor().spawn(future);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                done(this, result, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn persist_last_project(&self, id: Option<Id>, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = backend.set_last_project(id).await {
                    tracing::warn!(%error, "could not persist last project");
                }
            })
            .detach();
    }

    fn load_projects(&mut self, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let include_archived = self.show_archived_projects;
        self.run_backend(
            cx,
            async move { backend.list_projects(include_archived).await },
            |this, result, cx| match result {
                Ok(projects) => {
                    this.projects = projects;
                    if let Some(current) = this.current_project.as_ref()
                        && let Some(updated) = this
                            .projects
                            .iter()
                            .find(|project| project.id == current.id)
                            .cloned()
                    {
                        this.current_project = Some(updated);
                    }
                    cx.notify();
                }
                Err(error) => this.toast(ToastKind::Danger, error.message, cx),
            },
        );
    }

    fn load_environments(&mut self, project_id: Id, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let include_archived = self.show_archived_envs;
        self.run_backend(
            cx,
            async move {
                backend
                    .list_environments(project_id, include_archived)
                    .await
            },
            |this, result, cx| match result {
                Ok(environments) => {
                    this.environments = environments;
                    if !this
                        .selected_environment
                        .is_some_and(|id| this.environments.iter().any(|env| env.id == id))
                    {
                        this.selected_environment = this.environments.first().map(|env| env.id);
                    }
                    this.sync_env_select(cx);
                }
                Err(error) => this.toast(
                    ToastKind::Danger,
                    format!("Could not load environments: {}", error.message),
                    cx,
                ),
            },
        );
    }

    fn load_scenarios(&mut self, project_id: Id, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let include_archived = self.show_archived_scenarios;
        self.run_backend(
            cx,
            async move { backend.list_scenarios(project_id, include_archived).await },
            |this, result, cx| match result {
                Ok(scenarios) => {
                    this.scenarios = scenarios;
                }
                Err(error) => this.toast(
                    ToastKind::Danger,
                    format!("Could not load scenarios: {}", error.message),
                    cx,
                ),
            },
        );
    }

    fn sync_env_select(&mut self, cx: &mut Context<Self>) {
        let options: Vec<SharedString> = self
            .environments
            .iter()
            .map(|environment| {
                format!("{} · {}", environment.name, environment.kind.label()).into()
            })
            .collect();
        let selected = self
            .environments
            .iter()
            .position(|environment| Some(environment.id) == self.selected_environment)
            .unwrap_or(0);
        let placeholder = if self.environments.is_empty() {
            "No environments"
        } else {
            "Select environment"
        };
        self.env_select
            .update(cx, |select, cx| select.set_options(options, cx));
        self.env_select
            .update(cx, |select, cx| select.set_placeholder(placeholder, cx));
        self.env_select
            .update(cx, |select, cx| select.set_selected(selected, cx));
    }

    fn open_project(&mut self, id: Id, cx: &mut Context<Self>) {
        if let Some(project) = self
            .projects
            .iter()
            .find(|project| project.id == id)
            .cloned()
        {
            self.current_project = Some(project.clone());
            self.route = Route::Overview;
            self.modal = None;
            self.scenario_editor = None;
            self.pending_scenario = None;
            self.environments.clear();
            self.selected_environment = None;
            self.scenarios.clear();
            self.sync_env_select(cx);
            self.load_environments(project.id, cx);
            self.load_scenarios(project.id, cx);
            self.persist_last_project(Some(project.id), cx);
            cx.notify();
        }
    }

    fn toast(&mut self, kind: ToastKind, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        let id = self.next_toast_id;
        self.next_toast_id = self.next_toast_id.wrapping_add(1);
        self.toasts.push(ToastEntry {
            id,
            kind,
            message: message.into(),
        });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(4200))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.toasts.retain(|toast| toast.id != id);
                cx.notify();
            });
        })
        .detach();
    }

    fn dismiss_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|toast| toast.id != id);
        cx.notify();
    }

    // ---------------------------------------------------------------- opening

    fn open_create_project(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = text_input(cx, "e.g. Inventory API", "");
        let description = text_input(cx, "What this project covers (optional)", "");
        self.modal = Some(Modal::Project(ProjectModal {
            existing: None,
            name: name.clone(),
            description,
            accent: Accent::Blue,
            error: None,
            saving: false,
        }));
        window.focus(&name.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn open_edit_project(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.current_project.clone() else {
            return;
        };
        let name = text_input(cx, "Project name", &project.name);
        let description = text_input(cx, "Description", &project.description);
        self.modal = Some(Modal::Project(ProjectModal {
            existing: Some(project.clone()),
            name: name.clone(),
            description,
            accent: project.accent,
            error: None,
            saving: false,
        }));
        window.focus(&name.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn open_delete_project(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.current_project.clone() else {
            return;
        };
        let confirm = text_input(cx, format!("Type “{}” to confirm", project.name), "");
        self.modal = Some(Modal::DeleteProject(DeleteProjectModal {
            project,
            confirm: confirm.clone(),
            error: None,
        }));
        window.focus(&confirm.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn open_archive_project(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.current_project.clone() else {
            return;
        };
        self.modal = Some(Modal::ArchiveProject(ProjectRefModal { project }));
        cx.notify();
    }

    fn open_switch_project(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let filter = text_input(cx, "Filter projects…", "");
        let weak = cx.entity().downgrade();
        install_notify(&filter, weak, cx);
        self.modal = Some(Modal::SwitchProject(ProjectSwitcher {
            filter: filter.clone(),
        }));
        window.focus(&filter.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn open_environment(
        &mut self,
        existing: Option<Environment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.current_project.clone() else {
            return;
        };
        let environment = existing.as_ref();
        let kind = environment
            .map(|environment| environment.kind)
            .unwrap_or(EnvironmentKind::Local);
        let caps = environment
            .map(|environment| environment.caps.clone())
            .unwrap_or_else(|| kind.default_caps());
        let name = text_input(
            cx,
            "e.g. Staging API",
            environment
                .map(|environment| environment.name.as_str())
                .unwrap_or(""),
        );
        let base_url = text_input(
            cx,
            "https://api.example.com",
            environment
                .map(|environment| environment.base_url.as_str())
                .unwrap_or(""),
        );
        let auth_mode = environment
            .map(|environment| AuthMode::from_auth(&environment.auth))
            .unwrap_or(AuthMode::None);
        let auth_token_var = text_input(
            cx,
            "token (variable name)",
            auth_field(environment, |auth| match auth {
                AuthConfig::Bearer { token_var } => token_var.as_str(),
                _ => "",
            }),
        );
        let auth_username_var = text_input(
            cx,
            "username (variable name)",
            auth_field(environment, |auth| match auth {
                AuthConfig::Basic { username_var, .. } => username_var.as_str(),
                _ => "",
            }),
        );
        let auth_password_var = text_input(
            cx,
            "password (variable name)",
            auth_field(environment, |auth| match auth {
                AuthConfig::Basic { password_var, .. } => password_var.as_str(),
                _ => "",
            }),
        );
        let auth_header_name = text_input(
            cx,
            "X-API-Key",
            auth_field(environment, |auth| match auth {
                AuthConfig::Header { name, .. } => name.as_str(),
                _ => "",
            }),
        );
        let auth_header_value_var = text_input(
            cx,
            "api_key (variable name)",
            auth_field(environment, |auth| match auth {
                AuthConfig::Header { value_var, .. } => value_var.as_str(),
                _ => "",
            }),
        );
        let max_vus = text_input(cx, "500", caps.max_vus.to_string());
        let max_rps = text_input(
            cx,
            "No cap",
            caps.max_rps
                .map(|value| value.to_string())
                .unwrap_or_default(),
        );
        let max_duration = text_input(cx, "3600", caps.max_duration_s.to_string());
        let allowed_hosts = text_input(
            cx,
            "api.example.com, cdn.example.com:443",
            environment
                .map(|environment| environment.allowed_hosts.join(", "))
                .unwrap_or_default(),
        );
        let variables = environment
            .map(|environment| {
                environment
                    .variables
                    .iter()
                    .map(|variable| VariableRow::from_variable(variable, cx))
                    .collect()
            })
            .unwrap_or_default();

        let modal = EnvironmentModal {
            existing: existing.clone(),
            project_id: project.id,
            name: name.clone(),
            base_url,
            kind,
            auth_mode,
            auth_token_var,
            auth_username_var,
            auth_password_var,
            auth_header_name,
            auth_header_value_var,
            max_vus,
            max_rps,
            max_duration,
            require_confirmation: caps.require_confirmation,
            tls_verify: environment
                .map(|environment| environment.tls_verify)
                .unwrap_or(true),
            allow_external_hosts: environment
                .map(|environment| environment.allow_external_hosts)
                .unwrap_or(false),
            allowed_hosts,
            variables,
            error: None,
            saving: false,
        };
        self.modal = Some(Modal::Environment(Box::new(modal)));
        window.focus(&name.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn open_delete_environment(
        &mut self,
        environment: Environment,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.modal = Some(Modal::DeleteEnvironment(EnvironmentRefModal {
            environment,
        }));
        cx.notify();
    }

    fn open_archive_environment(
        &mut self,
        environment: Environment,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.modal = Some(Modal::ArchiveEnvironment(EnvironmentRefModal {
            environment,
        }));
        cx.notify();
    }

    fn open_settings(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.modal = Some(Modal::Settings);
        cx.notify();
    }

    fn close_modal(&mut self, _: &CloseModal, _: &mut Window, cx: &mut Context<Self>) {
        if self.modal.as_ref().is_some_and(Modal::dismissible) {
            self.modal = None;
            cx.notify();
        }
    }

    fn cancel_modal(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.modal.as_ref().is_some_and(Modal::dismissible) {
            self.modal = None;
            cx.notify();
        }
    }

    fn set_project_accent(&mut self, accent: Accent, cx: &mut Context<Self>) {
        if let Some(Modal::Project(form)) = self.modal.as_mut() {
            form.accent = accent;
            cx.notify();
        }
    }

    fn set_env_kind(&mut self, kind: EnvironmentKind, cx: &mut Context<Self>) {
        if let Some(Modal::Environment(form)) = self.modal.as_mut() {
            let old_defaults = form.kind.default_caps();
            let max_vus = form.max_vus.read(cx).content().trim().to_owned();
            let max_rps = form.max_rps.read(cx).content().trim().to_owned();
            let max_duration = form.max_duration.read(cx).content().trim().to_owned();
            let using_defaults = max_vus == old_defaults.max_vus.to_string()
                && max_rps
                    == old_defaults
                        .max_rps
                        .map(|value| value.to_string())
                        .unwrap_or_default()
                && max_duration == old_defaults.max_duration_s.to_string();
            form.kind = kind;
            if using_defaults {
                let defaults = kind.default_caps();
                form.max_vus.update(cx, |input, cx| {
                    input.set_content(defaults.max_vus.to_string(), cx)
                });
                form.max_rps.update(cx, |input, cx| {
                    input.set_content(
                        defaults
                            .max_rps
                            .map(|value| value.to_string())
                            .unwrap_or_default(),
                        cx,
                    )
                });
                form.max_duration.update(cx, |input, cx| {
                    input.set_content(defaults.max_duration_s.to_string(), cx)
                });
                form.require_confirmation = defaults.require_confirmation;
            } else if kind != EnvironmentKind::Local {
                form.require_confirmation = true;
            }
            cx.notify();
        }
    }

    fn set_auth_mode(&mut self, mode: AuthMode, cx: &mut Context<Self>) {
        if let Some(Modal::Environment(form)) = self.modal.as_mut() {
            form.auth_mode = mode;
            cx.notify();
        }
    }

    fn add_variable(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(Modal::Environment(form)) = self.modal.as_mut() {
            let key = text_input(cx, "API_KEY", "");
            let value = text_input(cx, "Value", "");
            form.variables.push(VariableRow {
                key,
                value,
                secret: false,
                secret_ref: None,
                existing_secret: false,
            });
            cx.notify();
        }
    }

    fn remove_variable(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(Modal::Environment(form)) = self.modal.as_mut()
            && index < form.variables.len()
        {
            form.variables.remove(index);
            cx.notify();
        }
    }

    fn toggle_variable_secret(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(Modal::Environment(form)) = self.modal.as_mut()
            && let Some(row) = form.variables.get_mut(index)
        {
            row.secret = !row.secret;
            let masked = row.secret;
            row.value
                .update(cx, |input, cx| input.set_masked(masked, cx));
            cx.notify();
        }
    }

    fn toggle_project_archived_filter(&mut self, cx: &mut Context<Self>) {
        self.show_archived_projects = !self.show_archived_projects;
        self.load_projects(cx);
    }

    fn toggle_env_archived_filter(&mut self, cx: &mut Context<Self>) {
        self.show_archived_envs = !self.show_archived_envs;
        if let Some(project) = self.current_project.as_ref() {
            self.load_environments(project.id, cx);
        }
    }

    fn toggle_env_tls(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(Modal::Environment(form)) = self.modal.as_mut() {
            form.tls_verify = !form.tls_verify;
            cx.notify();
        }
    }

    fn toggle_env_external_hosts(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(Modal::Environment(form)) = self.modal.as_mut() {
            form.allow_external_hosts = !form.allow_external_hosts;
            cx.notify();
        }
    }

    fn toggle_env_confirmation(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(Modal::Environment(form)) = self.modal.as_mut() {
            form.require_confirmation = !form.require_confirmation;
            cx.notify();
        }
    }

    // ---------------------------------------------------------------- saving

    fn save_project(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(Modal::Project(form)) = self.modal.as_mut() else {
            return;
        };
        if form.saving {
            return;
        }
        let name = form.name.read(cx).content().trim().to_owned();
        let description = form.description.read(cx).content().trim().to_owned();
        let accent = form.accent;
        let existing = form.existing.clone();
        let is_new = existing.is_none();
        let project = match existing {
            Some(mut project) => {
                project.name = name;
                project.description = description;
                project.accent = accent;
                project.updated_at = now();
                project
            }
            None => match Project::new(&name, &description, accent) {
                Ok(project) => project,
                Err(errors) => {
                    form.error = Some("Fix the highlighted fields.".into());
                    apply_project_errors(form, &errors, cx);
                    cx.notify();
                    return;
                }
            },
        };
        let errors = project.validate();
        if !errors.is_empty() {
            form.error = Some("Fix the highlighted fields.".into());
            apply_project_errors(form, &errors, cx);
            cx.notify();
            return;
        }
        form.saving = true;
        form.error = None;
        let backend = self.backend.clone();
        let project_id = project.id;
        self.run_backend(
            cx,
            async move {
                let saved = if is_new {
                    backend.create_project(project).await?
                } else {
                    backend.update_project(project).await?
                };
                let projects = backend.list_projects(false).await?;
                Ok((saved, projects))
            },
            move |this, result, cx| match result {
                Ok((saved, projects)) => {
                    this.projects = projects;
                    this.current_project = Some(saved.clone());
                    this.route = Route::Overview;
                    this.modal = None;
                    this.persist_last_project(Some(saved.id), cx);
                    if is_new {
                        this.environments.clear();
                        this.selected_environment = None;
                        this.sync_env_select(cx);
                        this.toast(ToastKind::Success, "Project created.", cx);
                    } else {
                        this.toast(ToastKind::Success, "Project saved.", cx);
                    }
                    let _ = project_id;
                }
                Err(error) => {
                    if let Some(Modal::Project(form)) = this.modal.as_mut() {
                        form.saving = false;
                        form.error = Some(error.message.clone().into());
                        apply_backend_project_errors(form, &error, cx);
                    }
                }
            },
        );
    }

    fn confirm_delete_project(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::DeleteProject(form)) = self.modal.as_mut() else {
            return;
        };
        let typed = form.confirm.read(cx).content().trim().to_owned();
        if typed != form.project.name {
            form.error =
                Some(format!("Type “{}” exactly to confirm deletion.", form.project.name).into());
            cx.notify();
            return;
        }
        let project = form.project.clone();
        let backend = self.backend.clone();
        self.run_backend(
            cx,
            async move {
                backend.delete_project(project.id).await?;
                let projects = backend.list_projects(false).await?;
                Ok((project.id, projects))
            },
            |this, result, cx| match result {
                Ok((deleted_id, projects)) => {
                    this.projects = projects;
                    this.modal = None;
                    if this
                        .current_project
                        .as_ref()
                        .is_some_and(|p| p.id == deleted_id)
                    {
                        this.current_project = None;
                        this.environments.clear();
                        this.selected_environment = None;
                        this.route = Route::Welcome;
                        this.persist_last_project(None, cx);
                        this.sync_env_select(cx);
                    }
                    this.toast(ToastKind::Success, "Project deleted.", cx);
                }
                Err(error) => {
                    if let Some(Modal::DeleteProject(form)) = this.modal.as_mut() {
                        form.error = Some(error.message.into());
                    }
                }
            },
        );
    }

    fn confirm_archive_project(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::ArchiveProject(form)) = self.modal.as_ref() else {
            return;
        };
        let project = form.project.clone();
        let project_id = project.id;
        let backend = self.backend.clone();
        self.run_backend(
            cx,
            async move {
                backend.archive_project(project_id, true).await?;
                backend.list_projects(false).await
            },
            move |this, result, cx| match result {
                Ok(projects) => {
                    this.projects = projects;
                    this.modal = None;
                    if this
                        .current_project
                        .as_ref()
                        .is_some_and(|p| p.id == project_id)
                    {
                        this.current_project = None;
                        this.environments.clear();
                        this.selected_environment = None;
                        this.route = Route::Welcome;
                        this.persist_last_project(None, cx);
                        this.sync_env_select(cx);
                    }
                    this.toast(
                        ToastKind::Success,
                        "Project archived. Restore it from the project switcher.",
                        cx,
                    );
                }
                Err(error) => this.toast(ToastKind::Danger, error.message, cx),
            },
        );
    }

    fn restore_project(&mut self, id: Id, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let include_archived = self.show_archived_projects;
        self.run_backend(
            cx,
            async move {
                backend.archive_project(id, false).await?;
                backend.list_projects(include_archived).await
            },
            |this, result, cx| match result {
                Ok(projects) => {
                    this.projects = projects;
                    this.toast(ToastKind::Success, "Project restored.", cx);
                }
                Err(error) => this.toast(ToastKind::Danger, error.message, cx),
            },
        );
    }

    fn save_environment(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(Modal::Environment(form)) = self.modal.as_mut() else {
            return;
        };
        if form.saving {
            return;
        }
        form.error = None;
        clear_env_errors(form, cx);

        let name = form.name.read(cx).content().trim().to_owned();
        let base_url = form.base_url.read(cx).content().trim().to_owned();
        let kind = form.kind;
        let mut field_errors: Vec<FieldError> = Vec::new();
        let mut variables: Vec<Variable> = Vec::new();
        let mut secrets: Vec<SecretUpdate> = Vec::new();

        for (index, row) in form.variables.iter_mut().enumerate() {
            let key = row.key.read(cx).content().trim().to_owned();
            let value = row.value.read(cx).content().to_owned();
            if key.is_empty() && value.is_empty() {
                continue;
            }
            if row.secret {
                let secret_ref = if let Some(secret_ref) = row.secret_ref {
                    secret_ref
                } else if value.is_empty() {
                    field_errors.push(FieldError::new(
                        format!("variables[{index}].value"),
                        "Enter a secret value.",
                    ));
                    Id::now_v7()
                } else {
                    let secret_ref = Id::now_v7();
                    row.secret_ref = Some(secret_ref);
                    secret_ref
                };
                if !value.is_empty() {
                    secrets.push(SecretUpdate { secret_ref, value });
                }
                variables.push(Variable::secret(key, secret_ref));
            } else {
                variables.push(Variable::plain(key, value));
            }
        }

        let max_vus: u32 = form.max_vus.read(cx).content().trim().parse().unwrap_or(0);
        let rps_text = form.max_rps.read(cx).content().trim().to_owned();
        let max_rps: Option<u32> = if rps_text.is_empty() {
            None
        } else {
            Some(rps_text.parse().unwrap_or(0))
        };
        let max_duration_s: u32 = form
            .max_duration
            .read(cx)
            .content()
            .trim()
            .parse()
            .unwrap_or(0);

        let auth = match form.auth_mode {
            AuthMode::None => AuthConfig::None,
            AuthMode::Bearer => AuthConfig::Bearer {
                token_var: form.auth_token_var.read(cx).content().trim().to_owned(),
            },
            AuthMode::Basic => AuthConfig::Basic {
                username_var: form.auth_username_var.read(cx).content().trim().to_owned(),
                password_var: form.auth_password_var.read(cx).content().trim().to_owned(),
            },
            AuthMode::Header => AuthConfig::Header {
                name: form.auth_header_name.read(cx).content().trim().to_owned(),
                value_var: form
                    .auth_header_value_var
                    .read(cx)
                    .content()
                    .trim()
                    .to_owned(),
            },
        };

        let allowed_hosts = form
            .allowed_hosts
            .read(cx)
            .content()
            .split(',')
            .map(str::trim)
            .filter(|host| !host.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();

        let existing = form.existing.clone();
        let mut environment = match &existing {
            Some(existing) => {
                let mut environment = existing.clone();
                environment.name = name;
                environment.base_url = base_url;
                environment.kind = kind;
                environment.variables = variables;
                environment.auth = auth;
                environment.tls_verify = form.tls_verify;
                environment.allow_external_hosts = form.allow_external_hosts;
                environment.allowed_hosts = allowed_hosts;
                environment.caps = SafetyCaps {
                    max_vus,
                    max_rps,
                    max_duration_s,
                    require_confirmation: form.require_confirmation,
                };
                environment.updated_at = now();
                environment
            }
            None => Environment {
                id: Id::now_v7(),
                project_id: form.project_id,
                name,
                kind,
                base_url,
                variables,
                auth,
                tls_verify: form.tls_verify,
                allow_external_hosts: form.allow_external_hosts,
                allowed_hosts,
                caps: SafetyCaps {
                    max_vus,
                    max_rps,
                    max_duration_s,
                    require_confirmation: form.require_confirmation,
                },
                revision: 1,
                created_at: now(),
                updated_at: now(),
                archived_at: None,
            },
        };
        environment.normalize();
        field_errors.extend(environment.validate());
        if !field_errors.is_empty() {
            form.error = Some("Fix the highlighted fields.".into());
            apply_env_errors(form, &field_errors, cx);
            cx.notify();
            return;
        }

        form.saving = true;
        let backend = self.backend.clone();
        let project_id = environment.project_id;
        let include_archived = self.show_archived_envs;
        self.run_backend(
            cx,
            async move {
                let saved = backend.save_environment(environment, secrets).await?;
                let environments = backend
                    .list_environments(project_id, include_archived)
                    .await?;
                Ok((saved, environments))
            },
            move |this, result, cx| match result {
                Ok((saved, environments)) => {
                    this.modal = None;
                    this.environments = environments;
                    this.selected_environment = Some(saved.id);
                    this.sync_env_select(cx);
                    this.toast(ToastKind::Success, "Environment saved.", cx);
                }
                Err(error) => {
                    if let Some(Modal::Environment(form)) = this.modal.as_mut() {
                        form.saving = false;
                        form.error = Some(error.message.clone().into());
                        apply_backend_env_errors(form, &error, cx);
                    }
                }
            },
        );
    }

    fn confirm_delete_environment(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::DeleteEnvironment(form)) = self.modal.as_ref() else {
            return;
        };
        let environment = form.environment.clone();
        let backend = self.backend.clone();
        let include_archived = self.show_archived_envs;
        self.run_backend(
            cx,
            async move {
                backend.delete_environment(environment.id).await?;
                backend
                    .list_environments(environment.project_id, include_archived)
                    .await
            },
            |this, result, cx| match result {
                Ok(environments) => {
                    this.modal = None;
                    this.environments = environments;
                    if !this
                        .selected_environment
                        .is_some_and(|id| this.environments.iter().any(|env| env.id == id))
                    {
                        this.selected_environment = this.environments.first().map(|env| env.id);
                    }
                    this.sync_env_select(cx);
                    this.toast(ToastKind::Success, "Environment deleted.", cx);
                }
                Err(error) => this.toast(
                    ToastKind::Danger,
                    format!("Could not delete environment: {}", error.message),
                    cx,
                ),
            },
        );
    }

    fn confirm_archive_environment(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::ArchiveEnvironment(form)) = self.modal.as_ref() else {
            return;
        };
        let environment = form.environment.clone();
        let archived = environment.archived_at.is_none();
        let backend = self.backend.clone();
        let include_archived = self.show_archived_envs;
        self.run_backend(
            cx,
            async move {
                backend
                    .archive_environment(environment.id, archived)
                    .await?;
                backend
                    .list_environments(environment.project_id, include_archived)
                    .await
            },
            move |this, result, cx| match result {
                Ok(environments) => {
                    this.modal = None;
                    this.environments = environments;
                    if archived && !this.show_archived_envs {
                        this.selected_environment = this.environments.first().map(|env| env.id);
                    } else {
                        this.selected_environment = Some(environment.id);
                    }
                    this.sync_env_select(cx);
                    this.toast(
                        ToastKind::Success,
                        if archived {
                            "Environment archived."
                        } else {
                            "Environment restored."
                        },
                        cx,
                    );
                }
                Err(error) => this.toast(ToastKind::Danger, error.message, cx),
            },
        );
    }

    fn duplicate_environment(&mut self, id: Id, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let include_archived = self.show_archived_envs;
        self.run_backend(
            cx,
            async move {
                let duplicate = backend.duplicate_environment(id).await?;
                let environments = backend
                    .list_environments(duplicate.project_id, include_archived)
                    .await?;
                Ok((duplicate, environments))
            },
            |this, result, cx| match result {
                Ok((duplicate, environments)) => {
                    this.environments = environments;
                    this.selected_environment = Some(duplicate.id);
                    this.sync_env_select(cx);
                    this.toast(ToastKind::Success, "Environment duplicated.", cx);
                }
                Err(error) => this.toast(
                    ToastKind::Danger,
                    format!("Could not duplicate: {}", error.message),
                    cx,
                ),
            },
        );
    }

    fn go_overview(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.current_project.is_some() {
            self.route = Route::Overview;
            self.scenario_editor = None;
            cx.notify();
        }
    }

    fn go_scenarios(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.current_project.is_some() {
            self.route = Route::Scenarios;
            self.scenario_editor = None;
            if let Some(project) = self.current_project.clone() {
                self.load_scenarios(project.id, cx);
            }
            cx.notify();
        }
    }

    fn open_scenario_editor(
        &mut self,
        scenario: Scenario,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(project) = self.current_project.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let environments = self.environments.clone();
        let selected_environment = self.selected_environment;
        let project_id = project.id;
        let shell = cx.entity().downgrade();
        let editor_shell = shell.clone();
        let saved_shell = shell;
        let editor = cx.new(move |cx| {
            let mut editor = ScenarioEditor::new(
                backend,
                project,
                environments,
                selected_environment,
                scenario,
                cx,
            );
            editor.set_callbacks(
                Box::new(move |cx| {
                    let _ = editor_shell.update(cx, |shell, cx| {
                        shell.scenario_editor = None;
                        shell.route = Route::Scenarios;
                        shell.load_scenarios(project_id, cx);
                        cx.notify();
                    });
                }),
                Box::new(move |saved, cx| {
                    let _ = saved_shell.update(cx, |shell, cx| {
                        shell.scenarios = shell
                            .scenarios
                            .iter()
                            .filter(|candidate| candidate.id != saved.id)
                            .cloned()
                            .chain(std::iter::once(saved.clone()))
                            .collect();
                        shell.scenarios.sort_by(|left, right| {
                            left.name.to_lowercase().cmp(&right.name.to_lowercase())
                        });
                        shell.load_scenarios(project_id, cx);
                        shell.toast(
                            ToastKind::Success,
                            format!("“{}” saved · revision {}", saved.name, saved.revision),
                            cx,
                        );
                    });
                }),
            );
            editor
        });
        self.scenario_editor = Some(editor.clone());
        self.route = Route::Scenarios;
        window.focus(&editor.read(cx).focus_handle(cx));
        cx.notify();
    }

    fn new_scenario(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.current_project.clone() else {
            return;
        };
        let scenarios = self.scenarios.clone();
        let mut name = "Untitled scenario".to_owned();
        let mut suffix = 2;
        while scenarios
            .iter()
            .any(|scenario| scenario.name.to_lowercase() == name.to_lowercase())
        {
            name = format!("Untitled scenario {suffix}");
            suffix += 1;
        }
        let scenario = Scenario::new(project.id, name);
        self.open_scenario_editor(scenario, window, cx);
    }

    fn duplicate_scenario(&mut self, id: Id, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        let include_archived = self.show_archived_scenarios;
        self.run_backend(
            cx,
            async move {
                let duplicate = backend.duplicate_scenario(id).await?;
                let scenarios = backend
                    .list_scenarios(duplicate.project_id, include_archived)
                    .await?;
                Ok((duplicate, scenarios))
            },
            |this, result, cx| match result {
                Ok((duplicate, scenarios)) => {
                    this.scenarios = scenarios;
                    this.toast(
                        ToastKind::Success,
                        format!("Duplicated as “{}”.", duplicate.name),
                        cx,
                    );
                }
                Err(error) => this.toast(
                    ToastKind::Danger,
                    format!("Could not duplicate: {}", error.message),
                    cx,
                ),
            },
        );
    }

    fn open_delete_scenario(
        &mut self,
        scenario: Scenario,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.modal = Some(Modal::DeleteScenario(ScenarioRefModal { scenario }));
        cx.notify();
    }

    fn open_archive_scenario(
        &mut self,
        scenario: Scenario,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.modal = Some(Modal::ArchiveScenario(ScenarioRefModal { scenario }));
        cx.notify();
    }

    fn confirm_delete_scenario(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::DeleteScenario(form)) = self.modal.take() else {
            return;
        };
        let backend = self.backend.clone();
        let project_id = form.scenario.project_id;
        let include_archived = self.show_archived_scenarios;
        self.run_backend(
            cx,
            async move {
                backend.delete_scenario(form.scenario.id).await?;
                backend.list_scenarios(project_id, include_archived).await
            },
            |this, result, cx| match result {
                Ok(scenarios) => {
                    this.scenarios = scenarios;
                    this.toast(ToastKind::Success, "Scenario deleted.", cx);
                }
                Err(error) => this.toast(ToastKind::Danger, error.message, cx),
            },
        );
    }

    fn confirm_archive_scenario(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Modal::ArchiveScenario(form)) = self.modal.take() else {
            return;
        };
        let archived = form.scenario.archived_at.is_none();
        let backend = self.backend.clone();
        let project_id = form.scenario.project_id;
        let include_archived = self.show_archived_scenarios;
        self.run_backend(
            cx,
            async move {
                backend.archive_scenario(form.scenario.id, archived).await?;
                backend.list_scenarios(project_id, include_archived).await
            },
            move |this, result, cx| match result {
                Ok(scenarios) => {
                    this.scenarios = scenarios;
                    this.toast(
                        ToastKind::Success,
                        if archived {
                            "Scenario archived."
                        } else {
                            "Scenario restored."
                        },
                        cx,
                    );
                }
                Err(error) => this.toast(ToastKind::Danger, error.message, cx),
            },
        );
    }

    fn toggle_scenario_archived_filter(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_archived_scenarios = !self.show_archived_scenarios;
        if let Some(project) = self.current_project.clone() {
            self.load_scenarios(project.id, cx);
        }
        cx.notify();
    }

    fn open_demo(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        self.run_backend(
            cx,
            async move {
                let demo = backend.open_demo().await?;
                let projects = backend.list_projects(true).await?;
                let project = projects
                    .iter()
                    .find(|project| project.id == demo.project_id)
                    .cloned()
                    .ok_or_else(|| BackendError::new("The demo project could not be loaded."))?;
                let environments = backend.list_environments(demo.project_id, false).await?;
                let scenarios = backend.list_scenarios(demo.project_id, false).await?;
                let scenario = scenarios
                    .iter()
                    .find(|scenario| scenario.id == demo.scenario_id)
                    .cloned()
                    .ok_or_else(|| BackendError::new("The demo scenario could not be loaded."))?;
                Ok((demo, projects, project, environments, scenarios, scenario))
            },
            |this, result, cx| match result {
                Ok((demo, projects, project, environments, scenarios, scenario)) => {
                    this.projects = projects;
                    this.current_project = Some(project.clone());
                    this.environments = environments;
                    this.scenarios = scenarios;
                    this.selected_environment = this
                        .environments
                        .iter()
                        .find(|environment| environment.base_url == demo.base_url)
                        .or_else(|| this.environments.first())
                        .map(|environment| environment.id);
                    this.sync_env_select(cx);
                    this.persist_last_project(Some(project.id), cx);
                    this.route = Route::Scenarios;
                    this.pending_scenario = Some(scenario);
                    this.toast(
                        ToastKind::Success,
                        "Demo project ready · mock target running on your machine.",
                        cx,
                    );
                }
                Err(error) => this.toast(
                    ToastKind::Danger,
                    format!("Could not open the demo: {}", error.message),
                    cx,
                ),
            },
        );
    }

    fn show_welcome(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.route = Route::Welcome;
        self.modal = None;
        self.pending_scenario = None;
        cx.notify();
    }

    // ---------------------------------------------------------------- views

    fn top_bar(&self, cx: &mut Context<Self>) -> Div {
        let current = self.current_project.as_ref();
        let project_label: SharedString = current
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "Select project".to_owned())
            .into();
        let accent = current
            .map(|project| theme::project_accent(project.accent))
            .unwrap_or_else(theme::text_muted);

        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(50.))
            .px(px(16.))
            .bg(theme::bg_surface())
            .border_b_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::show_welcome))
                            .cursor(gpui::CursorStyle::PointingHand)
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::text_primary())
                                    .child("BenchLab"),
                            )
                            .child(
                                div()
                                    .px(px(5.))
                                    .h(px(16.))
                                    .rounded(px(3.))
                                    .bg(theme::bg_raised())
                                    .text_size(px(8.))
                                    .text_color(theme::text_muted())
                                    .child("MVP"),
                            ),
                    )
                    .child(div().w(px(1.)).h(px(22.)).bg(theme::border_subtle()))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(30.))
                            .px(px(9.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme::border_default())
                            .bg(theme::bg_app())
                            .cursor(gpui::CursorStyle::PointingHand)
                            .hover(|style| style.bg(theme::bg_raised()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::open_switch_project),
                            )
                            .child(div().size(px(7.)).rounded_full().bg(accent))
                            .child(project_label)
                            .child(
                                div()
                                    .text_size(px(9.))
                                    .text_color(theme::text_muted())
                                    .child("▾"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(match self.route {
                                Route::Welcome => "Welcome",
                                Route::Overview => "Overview",
                                Route::Scenarios => "Scenarios",
                            }),
                    )
                    .child(div().w(px(260.)).child(self.search.clone())),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(24.))
                            .px(px(8.))
                            .rounded_full()
                            .bg(theme::bg_raised())
                            .child(div().size(px(6.)).rounded_full().bg(
                                if self.app.engine.ready() {
                                    theme::success()
                                } else {
                                    theme::warning()
                                },
                            ))
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(theme::text_secondary())
                                    .child(if self.app.engine.ready() {
                                        "k6 ready"
                                    } else {
                                        "Engine check"
                                    }),
                            ),
                    )
                    .when(self.route == Route::Overview, |this| {
                        this.child(disabled(button(
                            "New experiment",
                            ButtonVariant::Primary,
                            ButtonSize::Sm,
                        )))
                    }),
            )
    }

    fn rail(&self, cx: &mut Context<Self>) -> Div {
        let has_project = self.current_project.is_some();
        let overview_selected = self.route == Route::Overview;
        let scenarios_selected = self.route == Route::Scenarios;
        div()
            .flex()
            .flex_col()
            .w(px(216.))
            .h_full()
            .px(px(10.))
            .py(px(14.))
            .bg(theme::bg_surface())
            .border_r_1()
            .border_color(theme::border_subtle())
            .child(section_label("WORKSPACE"))
            .child(
                nav_item("Overview", overview_selected, has_project).when(has_project, |this| {
                    this.on_mouse_down(MouseButton::Left, cx.listener(Self::go_overview))
                }),
            )
            .child(nav_item("Experiments", false, false))
            .child(
                nav_item("Scenarios", scenarios_selected, has_project).when(has_project, |this| {
                    this.on_mouse_down(MouseButton::Left, cx.listener(Self::go_scenarios))
                }),
            )
            .child(nav_item("Profiles", false, false))
            .child(nav_item("Runs", false, false))
            .child(nav_item("Compare", false, false))
            .child(div().h(px(18.)))
            .child(section_label("SYSTEM"))
            .child(
                nav_item("Settings", false, true)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::open_settings)),
            )
            .child(div().flex_1())
            .child(self.engine_card())
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(9.))
                    .text_color(theme::text_muted())
                    .child(format!("v{}", self.app.app_version)),
            )
    }

    fn engine_card(&self) -> Div {
        let ready = self.app.engine.ready();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p(px(10.))
            .rounded(px(6.))
            .bg(theme::bg_app())
            .border_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(6.)).rounded_full().bg(if ready {
                        theme::success()
                    } else {
                        theme::warning()
                    }))
                    .child(
                        div()
                            .text_size(px(10.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_secondary())
                            .child("k6 engine"),
                    ),
            )
            .child(
                div()
                    .text_size(px(9.))
                    .text_color(theme::text_muted())
                    .child(self.app.engine.version.clone().unwrap_or_else(|| {
                        if self.app.engine.found {
                            "Unsupported version".to_owned()
                        } else {
                            "Not installed".to_owned()
                        }
                    })),
            )
    }

    fn content(&self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(editor) = self.scenario_editor.clone() {
            return editor.into_any_element();
        }
        if self.loading {
            return self.loading_view().into_any_element();
        }
        if let Some(error) = &self.load_error {
            return self.error_view(error.clone(), cx).into_any_element();
        }
        match self.route {
            Route::Welcome => self.welcome(cx),
            Route::Overview if self.current_project.is_some() => self.overview(cx),
            Route::Overview => self.welcome(cx),
            Route::Scenarios if self.current_project.is_some() => self.scenarios_view(cx),
            Route::Scenarios => self.welcome(cx),
        }
    }

    fn welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.search.read(cx).content().trim().to_lowercase();
        let projects: Vec<&Project> = self
            .projects
            .iter()
            .filter(|project| project.archived_at.is_none())
            .filter(|project| {
                query.is_empty()
                    || project.name.to_lowercase().contains(&query)
                    || project.description.to_lowercase().contains(&query)
            })
            .take(4)
            .collect();

        div()
            .id("welcome-scroll")
            .flex_1()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_5()
                    .w_full()
                    .max_w(px(900.))
                    .mx_auto()
                    .px(px(28.))
                    .py(px(52.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(30.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(theme::text_primary())
                                    .child("BenchLab"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::text_secondary())
                                    .child("A calm place to measure APIs, compare runs, and make performance decisions."),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .mt(px(8.))
                                    .child(
                                        button(
                                            "Create project",
                                            ButtonVariant::Primary,
                                            ButtonSize::Md,
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::open_create_project),
                                        ),
                                    )
                                    .child(
                                        button(
                                            "Try demo",
                                            ButtonVariant::Secondary,
                                            ButtonSize::Md,
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::open_demo),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .w_full()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(section_label("RECENT PROJECTS"))
                                    .child(
                                        row_action("New project").on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::open_create_project),
                                        ),
                                    ),
                            )
                            .when(projects.is_empty(), |this| {
                                this.child(
                                    empty_state(
                                        if query.is_empty() {
                                            "No projects yet"
                                        } else {
                                            "No matching projects"
                                        },
                                        if query.is_empty() {
                                            "Create a project to hold environments, experiments, and run history."
                                        } else {
                                            "Try a different project name."
                                        },
                                    )
                                    .bg(theme::bg_surface())
                                    .border_1()
                                    .border_color(theme::border_subtle())
                                    .rounded(px(8.)),
                                )
                            })
                            .children(projects.into_iter().map(|project| {
                                self.project_card(project, cx)
                            })),
                    )
                    .child(self.engine_strip()))
            .into_any_element()
    }

    fn project_card(&self, project: &Project, cx: &mut Context<Self>) -> Div {
        let project_id = project.id;
        div()
            .flex()
            .items_center()
            .gap_3()
            .w_full()
            .p(px(12.))
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::border_subtle())
            .rounded(px(8.))
            .hover(|style| style.border_color(theme::border_default()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.open_project(project_id, cx)),
            )
            .child(
                div()
                    .w(px(3.))
                    .h(px(38.))
                    .rounded_full()
                    .bg(theme::project_accent(project.accent)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .flex_1()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child(project.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(if project.description.is_empty() {
                                "No description".to_owned()
                            } else {
                                project.description.clone()
                            }),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::text_muted())
                    .child(format_relative(project.updated_at)),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::text_secondary())
                    .child("Open →"),
            )
    }

    fn engine_strip(&self) -> Div {
        let ready = self.app.engine.ready();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .p(px(12.))
            .rounded(px(8.))
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(6.)).rounded_full().bg(if ready {
                        theme::success()
                    } else {
                        theme::warning()
                    }))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_secondary())
                            .child("Load engine"),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(self.app.engine.summary()),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child(
                                self.app
                                    .engine
                                    .version
                                    .clone()
                                    .unwrap_or_else(|| "No version".to_owned()),
                            ),
                    ),
            )
            .when(!ready, |this| {
                this.child(banner(
                    BannerKind::Warning,
                    "k6 is required to run load tests. Install k6 2.x, then restart BenchLab.",
                ))
            })
    }

    fn scenarios_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(project) = self.current_project.as_ref() else {
            return self.welcome(cx);
        };
        let environment = self
            .selected_environment
            .and_then(|id| {
                self.environments
                    .iter()
                    .find(|environment| environment.id == id)
            })
            .or_else(|| self.environments.first());
        let active = self
            .scenarios
            .iter()
            .filter(|scenario| scenario.archived_at.is_none())
            .count();
        let rows: Vec<Div> = self
            .scenarios
            .iter()
            .map(|scenario| self.scenario_row(scenario, environment, cx))
            .collect();

        div()
            .id("scenarios-scroll")
            .flex_1()
            .overflow_y_scroll()
            .p(px(22.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w_full()
                    .max_w(px(1100.))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap_3()
                                    .child(
                                        div()
                                            .w(px(4.))
                                            .h(px(48.))
                                            .rounded_full()
                                            .bg(theme::project_accent(project.accent)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(px(24.))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(theme::text_primary())
                                                    .child("Scenarios"),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .text_color(theme::text_muted())
                                                    .child(format!(
                                                        "{} active scenario{} · ordered HTTP steps in {}",
                                                        active,
                                                        if active == 1 { "" } else { "s" },
                                                        project.name
                                                    )),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        row_action(if self.show_archived_scenarios {
                                            "Hide archived"
                                        } else {
                                            "Show archived"
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::toggle_scenario_archived_filter),
                                        ),
                                    )
                                    .child(
                                        button(
                                            "New scenario",
                                            ButtonVariant::Primary,
                                            ButtonSize::Sm,
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(Self::new_scenario),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(match environment {
                                Some(environment) => format!(
                                    "Validating against {} · {}",
                                    environment.name,
                                    environment.kind.label()
                                ),
                                None => "No environment yet · add one to validate targets and variables."
                                    .to_owned(),
                            }),
                    )
                    .child(
                        panel()
                            .child(panel_heading(
                                "Scenario library",
                                "Drafts are saved but cannot run until every step is valid.",
                            ))
                            .when(self.scenarios.is_empty(), |this| {
                                this.child(
                                    empty_state(
                                        "Scenarios are ordered HTTP steps",
                                        "Add the first request to begin. Steps can extract values for later requests and assert status, headers, bodies, duration, and size.",
                                    )
                                    .child(
                                        div().mt(px(6.)).child(
                                            button(
                                                "Add first step",
                                                ButtonVariant::Primary,
                                                ButtonSize::Sm,
                                            )
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(Self::new_scenario),
                                            ),
                                        ),
                                    ),
                                )
                            })
                            .children(rows)
                            .child(div().h(px(6.))),
                    ),
            )
            .into_any_element()
    }

    fn scenario_row(
        &self,
        scenario: &Scenario,
        environment: Option<&Environment>,
        cx: &mut Context<Self>,
    ) -> Div {
        let archived = scenario.archived_at.is_some();
        let issues = scenario.validate_with_environment(environment).len();
        let for_open = scenario.clone();
        let for_duplicate = scenario.clone();
        let for_archive = scenario.clone();
        let for_delete = scenario.clone();
        let path_summary = scenario
            .steps
            .iter()
            .map(|step| format!("{} {}", step.method.as_str(), step.url))
            .collect::<Vec<_>>()
            .join("  →  ");
        div()
            .flex()
            .flex_col()
            .gap_2()
            .mx(px(12.))
            .mb(px(8.))
            .p(px(12.))
            .rounded(px(6.))
            .bg(theme::bg_app())
            .border_1()
            .border_color(theme::border_subtle())
            .when(archived, |this| this.opacity(0.62))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child(scenario.name.clone()),
                    )
                    .when(archived, |this| this.child(neutral_badge("Archived")))
                    .when(issues == 0, |this| this.child(success_badge("Valid")))
                    .when(issues > 0, |this| {
                        this.child(warning_badge(format!(
                            "Draft · {issues} issue{}",
                            if issues == 1 { "" } else { "s" }
                        )))
                    })
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child(format!("rev {}", scenario.revision)),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child(format!(
                                "{} step{}",
                                scenario.steps.len(),
                                if scenario.steps.len() == 1 { "" } else { "s" }
                            )),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::text_muted())
                    .child(if path_summary.is_empty() {
                        "No steps".to_owned()
                    } else {
                        path_summary
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(row_action("Open").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.open_scenario_editor(for_open.clone(), window, cx)
                        }),
                    ))
                    .child(row_action("Duplicate").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.duplicate_scenario(for_duplicate.id, cx)
                        }),
                    ))
                    .child(
                        row_action(if archived { "Restore" } else { "Archive" }).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, window, cx| {
                                this.open_archive_scenario(for_archive.clone(), event, window, cx)
                            }),
                        ),
                    )
                    .child(row_action("Delete").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.open_delete_scenario(for_delete.clone(), event, window, cx)
                        }),
                    )),
            )
    }

    fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(project) = self.current_project.as_ref() else {
            return self.welcome(cx);
        };
        let environment_count = self.environments.len();
        let active_environment = self.selected_environment.and_then(|id| {
            self.environments
                .iter()
                .find(|environment| environment.id == id)
        });
        let environment_label = active_environment
            .map(|environment| format!("{} · {}", environment.name, environment.kind.label()))
            .unwrap_or_else(|| "No environment selected".to_owned());

        div()
            .id("overview-scroll")
            .flex_1()
            .overflow_y_scroll()
            .p(px(22.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .w_full()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap_3()
                                    .child(
                                        div()
                                            .w(px(4.))
                                            .h(px(48.))
                                            .rounded_full()
                                            .bg(theme::project_accent(project.accent)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(px(24.))
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_color(theme::text_primary())
                                                    .child(project.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.))
                                                    .text_color(theme::text_muted())
                                                    .child(if project.description.is_empty() {
                                                        "No project description".to_owned()
                                                    } else {
                                                        project.description.clone()
                                                    }),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(230.))
                                            .child(self.env_select.clone()),
                                    )
                                    .child(
                                        button(
                                            "Add environment",
                                            ButtonVariant::Secondary,
                                            ButtonSize::Sm,
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, window, cx| {
                                                this.open_environment(None, window, cx)
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(environment_label)
                            .child("·")
                            .child(format!(
                                "{} environment{}",
                                environment_count,
                                if environment_count == 1 { "" } else { "s" }
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .w_full()
                            .child(metric_card("Pass rate", "—", "No completed runs"))
                            .child(metric_card("Total runs", "0", "Run history is empty"))
                            .child(metric_card("Last run", "Never", "Nothing measured yet"))
                            .child(metric_card("Experiment", "None", "No experiment selected")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_3()
                            .w_full()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .flex_1()
                                    .child(
                                        panel()
                                            .child(panel_heading("p95 latency trend", "Completed runs only"))
                                            .child(
                                                empty_state(
                                                    "No trend yet",
                                                    "The p95 trend appears after this project has completed runs. BenchLab will never draw invented statistics.",
                                                ),
                                            ),
                                    )
                                    .child(
                                        panel()
                                            .child(panel_heading("Recent runs", "Latest completed and failed runs"))
                                            .child(recent_runs_table()),
                                    )
                                    .child(
                                        panel()
                                            .child(panel_heading("Experiments", "Saved test plans"))
                                            .child(
                                                empty_state(
                                                    "No experiments yet",
                                                    "Scenarios are ready in this milestone. Experiments add a load profile, objectives, and a baseline in the next one.",
                                                )
                                                .child(
                                                    div().mt(px(6.)).child(
                                                        button(
                                                            "Open scenarios",
                                                            ButtonVariant::Secondary,
                                                            ButtonSize::Sm,
                                                        )
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(Self::go_scenarios),
                                                        ),
                                                    ),
                                                ),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .w(px(340.))
                                    .flex_shrink_0()
                                    .child(self.environments_panel(cx))
                                    .child(self.project_actions_panel(cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn environments_panel(&self, cx: &mut Context<Self>) -> Div {
        let active_count = self
            .environments
            .iter()
            .filter(|environment| environment.archived_at.is_none())
            .count();
        panel()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(panel_heading(
                        "Environments",
                        format!(
                            "{} active{}",
                            active_count,
                            if self.show_archived_envs {
                                " · archived shown"
                            } else {
                                ""
                            }
                        ),
                    ))
                    .child(row_action("Add").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| this.open_environment(None, window, cx)),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(14.))
                    .pb(px(8.))
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child("TARGET CONFIGURATIONS"),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::accent())
                            .cursor(gpui::CursorStyle::PointingHand)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.toggle_env_archived_filter(cx)),
                            )
                            .child(if self.show_archived_envs {
                                "Hide archived"
                            } else {
                                "Show archived"
                            }),
                    ),
            )
            .when(self.environments.is_empty(), |this| {
                this.child(empty_state(
                    "No environments",
                    "Add a target so BenchLab can validate URLs, auth, and safety caps.",
                ))
            })
            .children(
                self.environments
                    .iter()
                    .map(|environment| self.environment_row(environment, cx)),
            )
            .child(div().h(px(8.)))
    }

    fn environment_row(&self, environment: &Environment, cx: &mut Context<Self>) -> Div {
        let archived = environment.archived_at.is_some();
        let host = environment
            .host()
            .unwrap_or_else(|| environment.base_url.clone());
        let environment_for_edit = environment.clone();
        let environment_for_duplicate = environment.clone();
        let environment_for_archive = environment.clone();
        let environment_for_delete = environment.clone();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .mx(px(12.))
            .mb(px(6.))
            .p(px(10.))
            .rounded(px(6.))
            .bg(theme::bg_app())
            .border_1()
            .border_color(theme::border_subtle())
            .when(archived, |this| this.opacity(0.62))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(environment_badge(environment.kind))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child(environment.name.clone()),
                    )
                    .when(archived, |this| this.child(neutral_badge("Archived")))
                    .child(
                        div()
                            .text_size(px(8.))
                            .text_color(theme::text_muted())
                            .child(format!("rev {}", environment.revision)),
                    ),
            )
            .child(
                div()
                    .text_size(px(9.))
                    .text_color(theme::text_muted())
                    .child(host),
            )
            .when(
                !environment.tls_verify || environment.allow_external_hosts,
                |this| {
                    this.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .when(!environment.tls_verify, |this| {
                                this.child(warning_badge("TLS verification off"))
                            })
                            .when(environment.allow_external_hosts, |this| {
                                this.child(warning_badge("External hosts allowed"))
                            }),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(row_action("Edit").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.open_environment(Some(environment_for_edit.clone()), window, cx)
                        }),
                    ))
                    .child(row_action("Duplicate").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.duplicate_environment(environment_for_duplicate.id, cx)
                        }),
                    ))
                    .child(
                        row_action(if archived { "Restore" } else { "Archive" }).on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, window, cx| {
                                this.open_archive_environment(
                                    environment_for_archive.clone(),
                                    event,
                                    window,
                                    cx,
                                )
                            }),
                        ),
                    )
                    .child(row_action("Delete").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.open_delete_environment(
                                environment_for_delete.clone(),
                                event,
                                window,
                                cx,
                            )
                        }),
                    )),
            )
    }

    fn project_actions_panel(&self, cx: &mut Context<Self>) -> Div {
        panel()
            .child(panel_heading("Project", "Manage this workspace"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px(px(8.))
                    .pb(px(8.))
                    .child(
                        row_action("Edit project")
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::open_edit_project)),
                    )
                    .child(
                        row_action("Archive project").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::open_archive_project),
                        ),
                    )
                    .child(
                        row_action("Delete project").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::open_delete_project),
                        ),
                    ),
            )
    }

    fn loading_view(&self) -> Div {
        div().flex_1().p(px(24.)).child(
            div()
                .flex()
                .flex_col()
                .gap_3()
                .max_w(px(900.))
                .mx_auto()
                .child(static_block(30., 220.))
                .child(static_block(14., 460.))
                .child(
                    div()
                        .flex()
                        .gap_3()
                        .child(static_block(96., 210.))
                        .child(static_block(96., 210.))
                        .child(static_block(96., 210.)),
                ),
        )
    }

    fn error_view(&self, error: SharedString, cx: &mut Context<Self>) -> Div {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .w(px(560.))
                    .p(px(20.))
                    .rounded(px(8.))
                    .bg(theme::bg_surface())
                    .border_1()
                    .border_color(theme::danger())
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child("Could not open the workspace"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::text_secondary())
                            .child(error),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                button("Retry", ButtonVariant::Primary, ButtonSize::Sm)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| this.load_initial(cx)),
                                    ),
                            )
                            .child(
                                button("Open settings", ButtonVariant::Secondary, ButtonSize::Sm)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(Self::open_settings),
                                    ),
                            ),
                    ),
            )
    }

    fn status_bar(&self) -> Div {
        let engine_ready = self.app.engine.ready();
        let secret_color = match self.app.secret_storage {
            SecretStorage::Keychain => theme::success(),
            SecretStorage::SessionOnly => theme::warning(),
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(24.))
            .px(px(12.))
            .bg(theme::bg_surface())
            .border_t_1()
            .border_color(theme::border_subtle())
            .text_size(px(9.))
            .text_color(theme::text_muted())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().size(px(5.)).rounded_full().bg(if engine_ready {
                                theme::success()
                            } else {
                                theme::warning()
                            }))
                            .child(if engine_ready {
                                "Engine ready"
                            } else {
                                "Engine not ready"
                            }),
                    )
                    .child("SQLite ready")
                    .child("No active run"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(div().size(px(5.)).rounded_full().bg(secret_color))
                            .child(format!("Secrets: {}", self.app.secret_storage.label())),
                    )
                    .child(self.app.data_dir_label())
                    .child(format!("v{}", self.app.app_version)),
            )
    }

    fn toast_overlay(&self, cx: &mut Context<Self>) -> Div {
        div()
            .absolute()
            .right(px(14.))
            .bottom(px(36.))
            .flex()
            .flex_col()
            .items_end()
            .gap_2()
            .children(self.toasts.iter().map(|toast| {
                let id = toast.id;
                toast_card(toast.message.clone(), toast.kind)
                    .id(("toast", toast.id as usize))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.dismiss_toast(id, cx)),
                    )
                    .into_any_element()
            }))
    }

    // ---------------------------------------------------------------- modal

    fn modal_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.modal.as_ref()?;
        let dismissible = modal.dismissible();
        let panel = match modal {
            Modal::Project(form) => self.project_modal(form, cx),
            Modal::DeleteProject(form) => self.delete_project_modal(form, cx),
            Modal::ArchiveProject(form) => self.archive_project_modal(form, cx),
            Modal::SwitchProject(form) => self.switch_project_modal(form, cx),
            Modal::Environment(form) => self.environment_modal(form, cx),
            Modal::DeleteEnvironment(form) => self.delete_environment_modal(form, cx),
            Modal::ArchiveEnvironment(form) => self.archive_environment_modal(form, cx),
            Modal::DeleteScenario(form) => self.delete_scenario_modal(form, cx),
            Modal::ArchiveScenario(form) => self.archive_scenario_modal(form, cx),
            Modal::Settings => self.settings_modal(cx),
        };

        Some(
            div()
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(px(22.))
                .bg(theme::bg_overlay())
                .occlude()
                .when(dismissible, |this| {
                    this.on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal))
                })
                .child(
                    div()
                        .on_mouse_down(
                            MouseButton::Left,
                            |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                                cx.stop_propagation();
                            },
                        )
                        .child(panel),
                )
                .into_any_element(),
        )
    }

    fn modal_frame(
        &self,
        title: impl Into<SharedString>,
        subtitle: Option<SharedString>,
        width: f32,
        body: Stateful<Div>,
        footer: Div,
        cx: &mut Context<Self>,
    ) -> Div {
        modal_panel(width)
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .child(modal_header(title, subtitle))
                    .child(
                        div()
                            .px(px(14.))
                            .pt(px(14.))
                            .text_size(px(14.))
                            .text_color(theme::text_muted())
                            .cursor(gpui::CursorStyle::PointingHand)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal))
                            .child("×"),
                    ),
            )
            .child(body)
            .child(footer)
    }

    fn project_modal(&self, form: &ProjectModal, cx: &mut Context<Self>) -> Div {
        let title = if form.existing.is_some() {
            "Edit project"
        } else {
            "New project"
        };
        let body = modal_body()
            .child(field("Name", None, form.name.clone()))
            .child(field("Description", None, form.description.clone()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(field_label("Accent", None))
                    .child(div().flex().items_center().gap_2().children(
                        Accent::ALL.into_iter().map(|accent| {
                            let selected = accent == form.accent;
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(26.))
                                .rounded_full()
                                .bg(theme::project_accent(accent))
                                .border_2()
                                .border_color(if selected {
                                    theme::text_primary()
                                } else {
                                    theme::bg_surface()
                                })
                                .cursor(gpui::CursorStyle::PointingHand)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, _, cx| {
                                        this.set_project_accent(accent, cx)
                                    }),
                                )
                                .when(selected, |this| {
                                    this.child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(theme::bg_sunken())
                                            .child("✓"),
                                    )
                                })
                        }),
                    )),
            )
            .when_some(form.error.clone(), |this, error| {
                this.child(banner(BannerKind::Danger, error))
            });

        let footer = modal_footer()
            .child(
                button("Cancel", ButtonVariant::Ghost, ButtonSize::Sm)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal)),
            )
            .child(
                button(
                    if form.existing.is_some() {
                        "Save changes"
                    } else {
                        "Create project"
                    },
                    ButtonVariant::Primary,
                    ButtonSize::Sm,
                )
                .on_mouse_down(MouseButton::Left, cx.listener(Self::save_project)),
            );

        self.modal_frame(
            title,
            Some("Projects group environments and run history.".into()),
            520.0,
            body,
            footer,
            cx,
        )
    }

    fn delete_project_modal(&self, form: &DeleteProjectModal, cx: &mut Context<Self>) -> Div {
        let body = modal_body()
            .child(banner(
                BannerKind::Danger,
                format!(
                    "This permanently deletes “{}” and every environment, experiment, run, and artifact inside it.",
                    form.project.name
                ),
            ))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_secondary())
                    .child("Type the project name exactly to confirm. This action cannot be undone."),
            )
            .child(field(
                "Project name",
                Some("Case-sensitive".into()),
                form.confirm.clone(),
            ))
            .when_some(form.error.clone(), |this, error| {
                this.child(inline_message(error, true))
            });
        let footer = modal_footer()
            .child(
                button("Cancel", ButtonVariant::Secondary, ButtonSize::Sm).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.modal = None;
                        cx.notify();
                    }),
                ),
            )
            .child(
                button("Delete project", ButtonVariant::Danger, ButtonSize::Sm)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::confirm_delete_project)),
            );
        modal_panel(520.0)
            .child(modal_header(
                "Delete project",
                Some("This is destructive and cannot be undone.".into()),
            ))
            .child(body)
            .child(footer)
    }

    fn archive_project_modal(&self, form: &ProjectRefModal, cx: &mut Context<Self>) -> Div {
        let body = modal_body()
            .child(banner(
                BannerKind::Warning,
                format!(
                    "Archive “{}”? It will be hidden from the default project list. All data is kept.",
                    form.project.name
                ),
            ))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child("Restore archived projects from the project switcher."),
            );
        let footer = modal_footer()
            .child(
                button("Cancel", ButtonVariant::Ghost, ButtonSize::Sm)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal)),
            )
            .child(
                button("Archive project", ButtonVariant::Danger, ButtonSize::Sm).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::confirm_archive_project),
                ),
            );
        modal_panel(500.0)
            .child(modal_header("Archive project", Some("Reversible".into())))
            .child(body)
            .child(footer)
    }

    fn switch_project_modal(&self, form: &ProjectSwitcher, cx: &mut Context<Self>) -> Div {
        let filter = form.filter.read(cx).content().trim().to_lowercase();
        let projects: Vec<Project> = self
            .projects
            .iter()
            .filter(|project| {
                filter.is_empty()
                    || project.name.to_lowercase().contains(&filter)
                    || project.description.to_lowercase().contains(&filter)
            })
            .cloned()
            .collect();

        let body = modal_body()
            .child(form.filter.clone())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(format!(
                                "{} project{}",
                                projects.len(),
                                if projects.len() == 1 { "" } else { "s" }
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(theme::text_muted())
                                    .child("Show archived"),
                            )
                            .child(
                                div()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.toggle_project_archived_filter(cx)
                                        }),
                                    )
                                    .child(checkbox(self.show_archived_projects)),
                            ),
                    ),
            )
            .when(projects.is_empty(), |this| {
                this.child(empty_state(
                    "No projects match",
                    "Clear the filter or create a new project.",
                ))
            })
            .children(projects.into_iter().map(|project| {
                let archived = project.archived_at.is_some();
                let project_id = project.id;
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .p(px(10.))
                    .rounded(px(6.))
                    .bg(theme::bg_app())
                    .border_1()
                    .border_color(theme::border_subtle())
                    .when(archived, |this| this.opacity(0.7))
                    .child(
                        div()
                            .w(px(3.))
                            .h(px(28.))
                            .rounded_full()
                            .bg(theme::project_accent(project.accent)),
                    )
                    .child(
                        div()
                            .flex_col()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_primary())
                                    .child(project.name),
                            )
                            .child(
                                div()
                                    .text_size(px(9.))
                                    .text_color(theme::text_muted())
                                    .child(format_relative(project.updated_at)),
                            ),
                    )
                    .when(archived, |this| this.child(neutral_badge("Archived")))
                    .child(if archived {
                        row_action("Restore").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| this.restore_project(project_id, cx)),
                        )
                    } else {
                        row_action("Open").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| this.open_project(project_id, cx)),
                        )
                    })
            }))
            .child(
                button("New project", ButtonVariant::Secondary, ButtonSize::Sm)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::open_create_project)),
            );

        let footer = modal_footer().child(
            button("Close", ButtonVariant::Ghost, ButtonSize::Sm)
                .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal)),
        );

        modal_panel(560.0)
            .child(modal_header(
                "Projects",
                Some("Open a project or manage archived workspaces.".into()),
            ))
            .child(body)
            .child(footer)
    }

    fn environment_modal(&self, form: &EnvironmentModal, cx: &mut Context<Self>) -> Div {
        let body = modal_body()
            .when_some(form.error.clone(), |this, error| {
                this.child(banner(BannerKind::Danger, error))
            })
            .child(
                div()
                    .flex()
                    .gap_3()
                    .w_full()
                    .child(div().flex_1().child(field("Name", None, form.name.clone())))
                    .child(
                        div()
                            .flex_1()
                            .child(field("Base URL", None, form.base_url.clone())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(field_label("Class", Some("Class sets conservative safety defaults.".into())))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(EnvironmentKind::ALL.into_iter().map(|kind| {
                                let selected = kind == form.kind;
                                div()
                                    .h(px(28.))
                                    .px(px(10.))
                                    .rounded(px(5.))
                                    .border_1()
                                    .border_color(if selected {
                                        theme::environment_accent(kind)
                                    } else {
                                        theme::border_default()
                                    })
                                    .bg(if selected {
                                        theme::tint(theme::environment_accent(kind))
                                    } else {
                                        theme::bg_app()
                                    })
                                    .text_size(px(11.))
                                    .text_color(if selected {
                                        theme::environment_accent(kind)
                                    } else {
                                        theme::text_secondary()
                                    })
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| this.set_env_kind(kind, cx)),
                                    )
                                    .child(kind.label())
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(field_label("Access", None))
                    .child(
                        toggle_row(
                            "Verify TLS certificates",
                            "Keep this on unless the target uses a trusted internal certificate.",
                            form.tls_verify,
                        )
                        .on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_env_tls)),
                    )
                    .child(
                        toggle_row(
                            "Allow external hosts",
                            "Host pinning is on by default. Only allow hosts you explicitly list.",
                            form.allow_external_hosts,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::toggle_env_external_hosts),
                        ),
                    )
                    .when(form.allow_external_hosts, |this| {
                        this.child(field(
                            "Allowed hosts",
                            Some("Comma-separated hosts, optional port.".into()),
                            form.allowed_hosts.clone(),
                        ))
                    })
                    .when(!form.tls_verify, |this| {
                        this.child(banner(
                            BannerKind::Warning,
                            "TLS verification is off for this environment. Connections are vulnerable to interception.",
                        ))
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(field_label(
                        "Authentication",
                        Some("Values are variable names. Never put a credential in a URL.".into()),
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(AuthMode::ALL.into_iter().map(|mode| {
                                let selected = mode == form.auth_mode;
                                div()
                                    .h(px(28.))
                                    .px(px(10.))
                                    .rounded(px(5.))
                                    .border_1()
                                    .border_color(if selected {
                                        theme::accent()
                                    } else {
                                        theme::border_default()
                                    })
                                    .bg(if selected {
                                        theme::tint(theme::accent())
                                    } else {
                                        theme::bg_app()
                                    })
                                    .text_size(px(11.))
                                    .text_color(if selected {
                                        theme::accent()
                                    } else {
                                        theme::text_secondary()
                                    })
                                    .cursor(gpui::CursorStyle::PointingHand)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.set_auth_mode(mode, cx)
                                        }),
                                    )
                                    .child(mode.label())
                            })),
                    )
                    .when(form.auth_mode == AuthMode::Bearer, |this| {
                        this.child(field(
                            "Token variable",
                            Some("Add this name as a secret variable below.".into()),
                            form.auth_token_var.clone(),
                        ))
                    })
                    .when(form.auth_mode == AuthMode::Basic, |this| {
                        this.child(
                            div()
                                .flex()
                                .gap_3()
                                .child(div().flex_1().child(field(
                                    "Username variable",
                                    None,
                                    form.auth_username_var.clone(),
                                )))
                                .child(div().flex_1().child(field(
                                    "Password variable",
                                    Some("Should be a secret variable.".into()),
                                    form.auth_password_var.clone(),
                                ))),
                        )
                    })
                    .when(form.auth_mode == AuthMode::Header, |this| {
                        this.child(
                            div()
                                .flex()
                                .gap_3()
                                .child(div().flex_1().child(field(
                                    "Header name",
                                    None,
                                    form.auth_header_name.clone(),
                                )))
                                .child(div().flex_1().child(field(
                                    "Value variable",
                                    Some("Should be a secret variable.".into()),
                                    form.auth_header_value_var.clone(),
                                ))),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(field_label(
                        "Safety caps",
                        Some("Hard ceilings for this environment. Preflight enforcement arrives with runs.".into()),
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .child(div().flex_1().child(field(
                                "Max VUs",
                                None,
                                form.max_vus.clone(),
                            )))
                            .child(div().flex_1().child(field(
                                "Max RPS",
                                Some("Optional".into()),
                                form.max_rps.clone(),
                            )))
                            .child(div().flex_1().child(field(
                                "Max duration (s)",
                                None,
                                form.max_duration.clone(),
                            ))),
                    )
                    .child(
                        toggle_row(
                            "Require confirmation",
                            "Ask before starting a run in this environment.",
                            form.require_confirmation,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::toggle_env_confirmation),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(field_label("Variables", None))
                            .child(
                                row_action("+ Add variable").on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::add_variable),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child("Secret values are stored in the OS keychain and never written to SQLite."),
                    )
                    .children(form.variables.iter().enumerate().map(|(index, row)| {
                        self.variable_row(index, row, cx)
                    })),
            );

        let footer = modal_footer()
            .child(
                button("Cancel", ButtonVariant::Ghost, ButtonSize::Sm)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal)),
            )
            .child(
                button(
                    if form.existing.is_some() {
                        "Save environment"
                    } else {
                        "Create environment"
                    },
                    ButtonVariant::Primary,
                    ButtonSize::Sm,
                )
                .on_mouse_down(MouseButton::Left, cx.listener(Self::save_environment)),
            );

        self.modal_frame(
            if form.existing.is_some() {
                "Edit environment"
            } else {
                "New environment"
            },
            Some("Targets, auth, variables, and safety limits.".into()),
            760.0,
            body,
            footer,
            cx,
        )
    }

    fn variable_row(&self, index: usize, row: &VariableRow, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_start()
            .gap_2()
            .w_full()
            .p(px(8.))
            .rounded(px(6.))
            .bg(theme::bg_app())
            .border_1()
            .border_color(theme::border_subtle())
            .child(div().w(px(170.)).child(row.key.clone()))
            .child(div().flex_1().child(row.value.clone()).when(
                row.existing_secret && row.secret,
                |this| {
                    this.child(
                        div()
                            .mt(px(3.))
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child("Stored in keychain. Leave blank to keep it."),
                    )
                },
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(32.))
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child("Secret"),
                    )
                    .child(
                        div()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.toggle_variable_secret(index, cx)
                                }),
                            )
                            .child(switch(row.secret)),
                    ),
            )
            .child(div().flex().items_center().h(px(32.)).child(
                row_action("Remove").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.remove_variable(index, cx)),
                ),
            ))
    }

    fn delete_environment_modal(&self, form: &EnvironmentRefModal, cx: &mut Context<Self>) -> Div {
        let body = modal_body()
            .child(banner(
                BannerKind::Danger,
                format!(
                    "Delete “{}”? Its secret metadata will be removed and keychain entries will be cleaned up.",
                    form.environment.name
                ),
            ))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child("Runs that reference this environment may prevent deletion once experiments exist."),
            );
        let footer = modal_footer()
            .child(
                button("Cancel", ButtonVariant::Secondary, ButtonSize::Sm).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.modal = None;
                        cx.notify();
                    }),
                ),
            )
            .child(
                button("Delete environment", ButtonVariant::Danger, ButtonSize::Sm).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::confirm_delete_environment),
                ),
            );
        modal_panel(520.0)
            .child(modal_header(
                "Delete environment",
                Some("This cannot be undone.".into()),
            ))
            .child(body)
            .child(footer)
    }

    fn archive_environment_modal(&self, form: &EnvironmentRefModal, cx: &mut Context<Self>) -> Div {
        let archived = form.environment.archived_at.is_some();
        let body = modal_body().child(banner(
            if archived {
                BannerKind::Info
            } else {
                BannerKind::Warning
            },
            if archived {
                format!("Restore “{}” to the active environment list?", form.environment.name)
            } else {
                format!(
                    "Archive “{}”? It will be hidden from the default list. Data and secrets are kept.",
                    form.environment.name
                )
            },
        ));
        let footer = modal_footer()
            .child(
                button("Cancel", ButtonVariant::Ghost, ButtonSize::Sm)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal)),
            )
            .child(
                button(
                    if archived {
                        "Restore environment"
                    } else {
                        "Archive environment"
                    },
                    if archived {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Danger
                    },
                    ButtonSize::Sm,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::confirm_archive_environment),
                ),
            );
        modal_panel(500.0)
            .child(modal_header(
                if archived {
                    "Restore environment"
                } else {
                    "Archive environment"
                },
                Some("Reversible".into()),
            ))
            .child(body)
            .child(footer)
    }

    fn delete_scenario_modal(&self, form: &ScenarioRefModal, cx: &mut Context<Self>) -> Div {
        let body = modal_body()
            .child(banner(
                BannerKind::Danger,
                format!(
                    "Delete “{}”? Its definition and revision history metadata are removed. Historical runs will keep their own snapshots once runs exist.",
                    form.scenario.name
                ),
            ))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child(format!(
                        "This scenario has {} step{}. Deleting cannot be undone.",
                        form.scenario.steps.len(),
                        if form.scenario.steps.len() == 1 { "" } else { "s" }
                    )),
            );
        let footer = modal_footer()
            .child(
                button("Cancel", ButtonVariant::Secondary, ButtonSize::Sm)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal)),
            )
            .child(
                button("Delete scenario", ButtonVariant::Danger, ButtonSize::Sm).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::confirm_delete_scenario),
                ),
            );
        modal_panel(520.0)
            .child(modal_header(
                "Delete scenario",
                Some("This cannot be undone.".into()),
            ))
            .child(body)
            .child(footer)
    }

    fn archive_scenario_modal(&self, form: &ScenarioRefModal, cx: &mut Context<Self>) -> Div {
        let archived = form.scenario.archived_at.is_some();
        let body = modal_body().child(banner(
            if archived {
                BannerKind::Info
            } else {
                BannerKind::Warning
            },
            if archived {
                format!(
                    "Restore “{}” to the active scenario list?",
                    form.scenario.name
                )
            } else {
                format!(
                    "Archive “{}”? It will be hidden from the default list. Its steps are kept.",
                    form.scenario.name
                )
            },
        ));
        let footer = modal_footer()
            .child(
                button("Cancel", ButtonVariant::Ghost, ButtonSize::Sm)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal)),
            )
            .child(
                button(
                    if archived {
                        "Restore scenario"
                    } else {
                        "Archive scenario"
                    },
                    if archived {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Danger
                    },
                    ButtonSize::Sm,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(Self::confirm_archive_scenario),
                ),
            );
        modal_panel(500.0)
            .child(modal_header(
                if archived {
                    "Restore scenario"
                } else {
                    "Archive scenario"
                },
                Some("Reversible".into()),
            ))
            .child(body)
            .child(footer)
    }

    fn settings_modal(&self, cx: &mut Context<Self>) -> Div {
        let engine = &self.app.engine;
        let body = modal_body()
            .child(banner(
                BannerKind::Info,
                "M2 scenario editor: build ordered HTTP steps, assert and extract values, and test every step against the demo target. Runs arrive in M4.",
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(info_row(
                        "Engine",
                        engine.summary(),
                        if engine.ready() {
                            theme::success()
                        } else {
                            theme::warning()
                        },
                    ))
                    .child(info_row(
                        "k6 path",
                        engine
                            .path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "Not found".to_owned()),
                        theme::text_secondary(),
                    ))
                    .child(info_row(
                        "Data directory",
                        self.app.data_dir.display().to_string(),
                        theme::text_secondary(),
                    ))
                    .child(info_row(
                        "Database",
                        self.app.db_path.display().to_string(),
                        theme::text_secondary(),
                    ))
                    .child(info_row(
                        "Secret storage",
                        self.app.secret_storage.label().to_owned(),
                        match self.app.secret_storage {
                            SecretStorage::Keychain => theme::success(),
                            SecretStorage::SessionOnly => theme::warning(),
                        },
                    )),
            )
            .when(
                self.app.secret_storage == SecretStorage::SessionOnly,
                |this| {
                    this.child(banner(
                        BannerKind::Warning,
                        "The OS keychain is unavailable. Secrets are kept for this session only and will need to be re-entered after restart.",
                    ))
                },
            );
        let footer = modal_footer().child(
            button("Close", ButtonVariant::Primary, ButtonSize::Sm)
                .on_mouse_down(MouseButton::Left, cx.listener(Self::cancel_modal)),
        );
        modal_panel(620.0)
            .child(modal_header(
                "Settings",
                Some("Local workspace and engine status.".into()),
            ))
            .child(body)
            .child(footer)
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(scenario) = self.pending_scenario.take() {
            let weak = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                let _ = weak.update(cx, |shell, cx| {
                    shell.open_scenario_editor(scenario, window, cx);
                });
            });
        }
        let top = self.top_bar(cx);
        let rail = self.rail(cx);
        let content = self.content(cx);
        let status = self.status_bar();
        let modal = self.modal_overlay(cx);
        let toasts = self.toast_overlay(cx);

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg_app())
            .text_color(theme::text_primary())
            .key_context("BenchLabShell")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::close_modal))
            .child(top)
            .child(
                div().flex().flex_1().overflow_hidden().child(rail).child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .overflow_hidden()
                        .child(content),
                ),
            )
            .child(status);

        if let Some(modal) = modal {
            root = root.child(modal);
        }
        root.child(toasts)
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

// ------------------------------------------------------------------ helpers

fn text_input(
    cx: &mut Context<Shell>,
    placeholder: impl AsRef<str>,
    content: impl AsRef<str>,
) -> Entity<TextInput> {
    let placeholder: SharedString = placeholder.as_ref().to_owned().into();
    let content: SharedString = content.as_ref().to_owned().into();
    cx.new(move |cx| {
        let mut input = TextInput::new(placeholder, cx);
        if !content.is_empty() {
            input.set_content(content, cx);
        }
        input
    })
}

fn install_notify(input: &Entity<TextInput>, shell: WeakEntity<Shell>, cx: &mut Context<Shell>) {
    input.update(cx, |input, _| {
        input.set_on_change(Some(Box::new(move |_, _window, cx| {
            let _ = shell.update(cx, |_, cx| cx.notify());
        })))
    });
}

impl VariableRow {
    fn from_variable(variable: &Variable, cx: &mut Context<Shell>) -> Self {
        let key = text_input(cx, "KEY", &variable.key);
        let value = text_input(cx, "Value", variable.value.as_deref().unwrap_or(""));
        let row = Self {
            key,
            value,
            secret: variable.secret,
            secret_ref: variable.secret_ref,
            existing_secret: variable.secret,
        };
        if variable.secret {
            row.value.update(cx, |input, cx| input.set_masked(true, cx));
        }
        row
    }
}

fn auth_field<'a>(
    environment: Option<&'a Environment>,
    field: impl Fn(&'a AuthConfig) -> &'a str,
) -> &'a str {
    environment
        .map(|environment| field(&environment.auth))
        .unwrap_or("")
}

fn apply_project_errors(form: &mut ProjectModal, errors: &[FieldError], cx: &mut Context<Shell>) {
    form.name.update(cx, |input, cx| input.set_error(None, cx));
    form.description
        .update(cx, |input, cx| input.set_error(None, cx));
    for error in errors {
        let message: SharedString = error.message.clone().into();
        match error.field.as_str() {
            "name" => form
                .name
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "description" => form
                .description
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            _ => {}
        }
    }
}

fn apply_backend_project_errors(
    form: &mut ProjectModal,
    error: &BackendError,
    cx: &mut Context<Shell>,
) {
    form.name.update(cx, |input, cx| input.set_error(None, cx));
    form.description
        .update(cx, |input, cx| input.set_error(None, cx));
    if let Some(message) = error.field_message("name") {
        form.name
            .update(cx, |input, cx| input.set_error(Some(message), cx));
    }
    if let Some(message) = error.field_message("description") {
        form.description
            .update(cx, |input, cx| input.set_error(Some(message), cx));
    }
}

fn clear_env_errors(form: &mut EnvironmentModal, cx: &mut Context<Shell>) {
    for input in [
        &form.name,
        &form.base_url,
        &form.auth_token_var,
        &form.auth_username_var,
        &form.auth_password_var,
        &form.auth_header_name,
        &form.auth_header_value_var,
        &form.max_vus,
        &form.max_rps,
        &form.max_duration,
        &form.allowed_hosts,
    ] {
        input.update(cx, |input, cx| input.set_error(None, cx));
    }
    for row in &mut form.variables {
        row.key.update(cx, |input, cx| input.set_error(None, cx));
        row.value.update(cx, |input, cx| input.set_error(None, cx));
    }
}

fn apply_env_errors(form: &mut EnvironmentModal, errors: &[FieldError], cx: &mut Context<Shell>) {
    for error in errors {
        let message: SharedString = error.message.clone().into();
        match error.field.as_str() {
            "name" => form
                .name
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "base_url" => form
                .base_url
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "caps.max_vus" => form
                .max_vus
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "caps.max_rps" => form
                .max_rps
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "caps.max_duration_s" => form
                .max_duration
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "allowed_hosts" => form
                .allowed_hosts
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "auth.token_var" => form
                .auth_token_var
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "auth.username_var" => form
                .auth_username_var
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "auth.password_var" => form
                .auth_password_var
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "auth.name" => form
                .auth_header_name
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            "auth.value_var" => form
                .auth_header_value_var
                .update(cx, |input, cx| input.set_error(Some(message), cx)),
            field => {
                if let Some(index) = variable_index(field)
                    && let Some(row) = form.variables.get_mut(index)
                {
                    if field.ends_with(".key") {
                        row.key
                            .update(cx, |input, cx| input.set_error(Some(message), cx));
                    } else if field.ends_with(".value") {
                        row.value
                            .update(cx, |input, cx| input.set_error(Some(message), cx));
                    }
                }
            }
        }
    }
}

fn apply_backend_env_errors(
    form: &mut EnvironmentModal,
    error: &BackendError,
    cx: &mut Context<Shell>,
) {
    if !error.fields.is_empty() {
        apply_env_errors(form, &error.fields, cx);
    }
}

fn variable_index(field: &str) -> Option<usize> {
    field
        .strip_prefix("variables[")?
        .split(']')
        .next()?
        .parse()
        .ok()
}

fn section_label(label: impl Into<SharedString>) -> Div {
    div()
        .h(px(24.))
        .px(px(8.))
        .flex()
        .items_center()
        .text_size(px(9.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_muted())
        .child(label.into())
}

fn nav_item(label: &'static str, selected: bool, enabled: bool) -> Div {
    let mut item = div()
        .flex()
        .items_center()
        .gap_2()
        .h(px(32.))
        .px(px(8.))
        .rounded(px(5.))
        .text_size(px(11.))
        .cursor(if enabled {
            gpui::CursorStyle::PointingHand
        } else {
            gpui::CursorStyle::Arrow
        })
        .text_color(if selected {
            theme::text_primary()
        } else if enabled {
            theme::text_secondary()
        } else {
            theme::text_muted()
        })
        .bg(if selected {
            theme::bg_raised()
        } else {
            theme::bg_surface()
        })
        .when(selected, |this| {
            this.child(
                div()
                    .w(px(2.))
                    .h(px(14.))
                    .rounded_full()
                    .bg(theme::accent()),
            )
        })
        .child(label);
    if enabled {
        item = item.hover(|style| style.bg(theme::bg_raised()));
    } else {
        item = item.opacity(0.52);
    }
    item
}

fn panel() -> Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .bg(theme::bg_surface())
        .border_1()
        .border_color(theme::border_subtle())
        .rounded(px(8.))
}

fn panel_heading(title: impl Into<SharedString>, subtitle: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p(px(14.))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_primary())
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(9.))
                .text_color(theme::text_muted())
                .child(subtitle.into()),
        )
}

fn metric_card(label: &'static str, value: &'static str, hint: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .flex_1()
        .p(px(14.))
        .rounded(px(8.))
        .bg(theme::bg_surface())
        .border_1()
        .border_color(theme::border_subtle())
        .child(
            div()
                .text_size(px(9.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_muted())
                .child(label),
        )
        .child(
            div()
                .text_size(px(24.))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text_primary())
                .child(value),
        )
        .child(
            div()
                .text_size(px(9.))
                .text_color(theme::text_muted())
                .child(hint),
        )
}

fn recent_runs_table() -> Div {
    let columns = [
        Column::fixed("Verdict", 90.),
        Column::flex("Experiment"),
        Column::fixed("Environment", 130.),
        Column::fixed("Started", 130.),
        Column::fixed("p95", 90.),
    ];
    div()
        .flex()
        .flex_col()
        .w_full()
        .child(table_header(&columns))
        .child(
            div()
                .flex()
                .items_center()
                .w_full()
                .min_h(px(76.))
                .px(px(12.))
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child("No runs yet. BenchLab will list completed and failed runs here."),
        )
}

fn info_row(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    color: gpui::Rgba,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p(px(9.))
        .rounded(px(5.))
        .bg(theme::bg_app())
        .border_1()
        .border_color(theme::border_subtle())
        .child(
            div()
                .text_size(px(9.))
                .text_color(theme::text_muted())
                .child(label.into()),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(color)
                .child(value.into()),
        )
}

fn toggle_row(label: &'static str, description: &'static str, checked: bool) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .w_full()
        .p(px(9.))
        .rounded(px(5.))
        .bg(theme::bg_app())
        .border_1()
        .border_color(theme::border_subtle())
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|style| style.bg(theme::bg_raised()))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::text_secondary())
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(9.))
                        .text_color(theme::text_muted())
                        .child(description),
                ),
        )
        .child(switch(checked))
}

fn static_block(height: f32, width: f32) -> Div {
    div()
        .h(px(height))
        .w(px(width))
        .rounded(px(6.))
        .bg(theme::bg_raised())
}

fn format_relative(timestamp: DateTime<Utc>) -> String {
    let seconds = Utc::now().signed_duration_since(timestamp).num_seconds();
    if seconds < 60 {
        "just now".to_owned()
    } else if seconds < 3_600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3_600)
    } else if seconds < 604_800 {
        format!("{}d ago", seconds / 86_400)
    } else {
        timestamp.with_timezone(&Local).format("%b %d").to_string()
    }
}
