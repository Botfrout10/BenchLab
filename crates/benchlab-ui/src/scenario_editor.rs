//! Scenario editor (SPEC.md §6.3 and §7.7).
//!
//! The editor owns one scenario draft, its step list, the inspector forms, the
//! variables panel, and the single-request tester. It talks only to
//! `BenchLabBackend`; the shell swaps it in when a scenario is opened.

use std::sync::Arc;

use benchlab_core::{
    Assertion, Body, Environment, Extraction, FieldError, Id, KeyValue, Method, Project, Scenario,
    ScenarioVariable, Step, ThinkTime, parse_scalar_input, scenario::json_scalar_text,
};
use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, Entity, FocusHandle, Focusable, FontWeight,
    MouseButton, MouseDownEvent, Render, SharedString, WeakEntity, Window, div, prelude::*, px,
};
use serde_json::Value;

use crate::{
    backend::{BenchLabBackend, TestStepRequest, TestStepResponse},
    components::{
        BannerKind, ButtonSize, ButtonVariant, Select, TextInput, banner, button, empty_state,
        field_label, inline_message, modal_body, modal_footer, modal_header, modal_panel,
        neutral_badge, row_action, success_badge, warning_badge,
    },
    theme,
};

type CloseCallback = Box<dyn Fn(&mut App)>;
type SaveCallback = Box<dyn Fn(Scenario, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepTab {
    Headers,
    Query,
    Body,
    Assertions,
    Extract,
}

impl StepTab {
    const ALL: [StepTab; 5] = [
        StepTab::Headers,
        StepTab::Query,
        StepTab::Body,
        StepTab::Assertions,
        StepTab::Extract,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseTab {
    Headers,
    Body,
}

pub struct ScenarioEditor {
    backend: Arc<dyn BenchLabBackend>,
    project: Project,
    environments: Vec<Environment>,
    env_select: Entity<Select>,
    selected_environment: Option<Id>,
    scenario: Scenario,
    name: Entity<TextInput>,
    description: Entity<TextInput>,
    selected_step: usize,
    tab: StepTab,
    form: Option<StepForm>,
    variables: Vec<VariableRow>,
    show_variables: bool,
    confirm_close: bool,
    dirty: bool,
    saving: bool,
    save_error: Option<SharedString>,
    assertion_kind: Entity<Select>,
    extraction_kind: Entity<Select>,
    testing: bool,
    test_error: Option<SharedString>,
    response: Option<TestStepResponse>,
    response_tab: ResponseTab,
    extra_variables: Vec<ScenarioVariable>,
    notice: Option<SharedString>,
    on_close: Option<CloseCallback>,
    on_saved: Option<SaveCallback>,
}

struct StepForm {
    index: usize,
    name: Entity<TextInput>,
    url: Entity<TextInput>,
    timeout: Entity<TextInput>,
    expected_status: Entity<TextInput>,
    method: Entity<Select>,
    think_a: Entity<TextInput>,
    think_b: Entity<TextInput>,
    body: Entity<TextInput>,
    headers: Vec<KeyValueRow>,
    query: Vec<KeyValueRow>,
    assertions: Vec<AssertionRow>,
    extractions: Vec<ExtractionRow>,
}

struct KeyValueRow {
    key: Entity<TextInput>,
    value: Entity<TextInput>,
}

struct VariableRow {
    key: Entity<TextInput>,
    value: Entity<TextInput>,
}

enum AssertionRow {
    HeaderEquals {
        name: Entity<TextInput>,
        value: Entity<TextInput>,
    },
    BodyContains {
        value: Entity<TextInput>,
    },
    BodyRegex {
        pattern: Entity<TextInput>,
    },
    JsonPathEquals {
        path: Entity<TextInput>,
        value: Entity<TextInput>,
    },
    MaxDuration {
        value: Entity<TextInput>,
    },
    MaxSize {
        value: Entity<TextInput>,
    },
}

enum ExtractionRow {
    JsonPath {
        variable: Entity<TextInput>,
        path: Entity<TextInput>,
    },
    Regex {
        variable: Entity<TextInput>,
        pattern: Entity<TextInput>,
        group: Entity<TextInput>,
    },
    Header {
        variable: Entity<TextInput>,
        name: Entity<TextInput>,
    },
    Status {
        variable: Entity<TextInput>,
    },
}

impl ScenarioEditor {
    pub fn new(
        backend: Arc<dyn BenchLabBackend>,
        project: Project,
        environments: Vec<Environment>,
        initial_environment: Option<Id>,
        scenario: Scenario,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak = cx.entity().downgrade();
        let selected_environment = initial_environment
            .filter(|id| environments.iter().any(|environment| environment.id == *id))
            .or_else(|| environments.first().map(|environment| environment.id));

        let name = text_input(cx, "Scenario name", &scenario.name);
        bind_text(&name, &weak, cx, |this, text| {
            this.scenario.name = text.to_owned();
        });
        let description = text_input(
            cx,
            "What this scenario proves (optional)",
            &scenario.description,
        );
        bind_text(&description, &weak, cx, |this, text| {
            this.scenario.description = text.to_owned();
        });

        let options: Vec<SharedString> = environments
            .iter()
            .map(|environment| {
                format!("{} · {}", environment.name, environment.kind.label()).into()
            })
            .collect();
        let selected = environments
            .iter()
            .position(|environment| Some(environment.id) == selected_environment)
            .unwrap_or(0);
        let env_select = cx.new(|cx| Select::new(options, selected, cx));
        bind_env_select(&env_select, &weak, cx);

        let assertion_kind = cx.new(|cx| {
            Select::new(
                [
                    "Header equals",
                    "Body contains",
                    "Body regex",
                    "JSON path equals",
                    "Max duration",
                    "Max size",
                ],
                0,
                cx,
            )
        });
        let extraction_kind =
            cx.new(|cx| Select::new(["JSON path", "Regex group", "Header", "Status"], 0, cx));

        let selected_step = 0;
        let variables = Vec::new();

        let mut editor = Self {
            backend,
            project,
            environments,
            env_select,
            selected_environment,
            scenario,
            name,
            description,
            selected_step,
            tab: StepTab::Headers,
            form: None,
            variables,
            show_variables: false,
            confirm_close: false,
            dirty: false,
            saving: false,
            save_error: None,
            assertion_kind,
            extraction_kind,
            testing: false,
            test_error: None,
            response: None,
            response_tab: ResponseTab::Body,
            extra_variables: Vec::new(),
            notice: None,
            on_close: None,
            on_saved: None,
        };
        editor.rebuild_all(cx);
        editor
    }

    pub fn set_callbacks(&mut self, on_close: CloseCallback, on_saved: SaveCallback) {
        self.on_close = Some(on_close);
        self.on_saved = Some(on_saved);
    }

    pub fn focus_name(&self, window: &mut Window, cx: &App) {
        window.focus(&self.name.read(cx).focus_handle(cx));
    }

    fn rebuild_all(&mut self, cx: &mut Context<Self>) {
        self.clamp_selection();
        self.rebuild_form(cx);
        self.rebuild_variables(cx);
    }

    fn clamp_selection(&mut self) {
        if self.scenario.steps.is_empty() {
            self.selected_step = 0;
        } else if self.selected_step >= self.scenario.steps.len() {
            self.selected_step = self.scenario.steps.len() - 1;
        }
    }

    fn selected_environment(&self) -> Option<&Environment> {
        self.selected_environment.and_then(|id| {
            self.environments
                .iter()
                .find(|environment| environment.id == id)
        })
    }

    fn validation_errors(&self) -> Vec<FieldError> {
        self.scenario
            .validate_with_environment(self.selected_environment())
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.save_error = None;
        self.test_error = None;
    }

    fn rebuild_form(&mut self, cx: &mut Context<Self>) {
        let Some(step) = self.scenario.steps.get(self.selected_step).cloned() else {
            self.form = None;
            return;
        };
        let weak = cx.entity().downgrade();
        let index = self.selected_step;

        let name = text_input(cx, "Step name", &step.name);
        bind_step_text(&name, &weak, cx, index, |step, text| {
            step.name = text.to_owned();
        });
        let url = text_input(cx, "{{base_url}}/items or /items", &step.url);
        bind_step_text(&url, &weak, cx, index, |step, text| {
            step.url = text.to_owned();
        });
        let timeout = text_input(cx, "30000", step.timeout_ms.to_string());
        bind_step_text(&timeout, &weak, cx, index, |step, text| {
            step.timeout_ms = parse_u64(text);
        });
        let expected_status = text_input(cx, "200", join_statuses(&step.expected_status));
        bind_step_text(&expected_status, &weak, cx, index, |step, text| {
            step.expected_status = parse_statuses(text);
        });

        let method = cx.new(|cx| {
            let options: Vec<SharedString> = Method::ALL
                .iter()
                .map(|method| SharedString::from(method.as_str()))
                .collect();
            let selected = Method::ALL
                .iter()
                .position(|method| *method == step.method)
                .unwrap_or(0);
            Select::new(options, selected, cx)
        });
        let method_weak = weak.clone();
        method.update(cx, |select, _| {
            select.set_on_change(Some(Box::new(move |choice, _window, cx| {
                let method = Method::ALL.get(choice).copied().unwrap_or(Method::Get);
                let _ = method_weak.update(cx, |this, cx| {
                    if let Some(step) = this.scenario.steps.get_mut(index) {
                        step.method = method;
                        if !method.allows_body() {
                            step.body = Body::None;
                        }
                    }
                    this.mark_dirty();
                    this.rebuild_form(cx);
                    cx.notify();
                });
            })));
        });

        let (think_a_text, think_b_text) = match step.think_time {
            ThinkTime::None => (String::new(), String::new()),
            ThinkTime::Fixed { ms } => (ms.to_string(), String::new()),
            ThinkTime::Range { min_ms, max_ms } => (min_ms.to_string(), max_ms.to_string()),
        };
        let think_a = text_input(cx, "ms", think_a_text);
        bind_step_text(&think_a, &weak, cx, index, |step, text| {
            let value = parse_u64(text);
            step.think_time = match step.think_time {
                ThinkTime::Range { max_ms, .. } => ThinkTime::Range {
                    min_ms: value,
                    max_ms,
                },
                _ => ThinkTime::Fixed { ms: value },
            };
        });
        let think_b = text_input(cx, "max ms", think_b_text);
        bind_step_text(&think_b, &weak, cx, index, |step, text| {
            let value = parse_u64(text);
            if let ThinkTime::Range { min_ms, .. } = step.think_time {
                step.think_time = ThinkTime::Range {
                    min_ms,
                    max_ms: value,
                };
            }
        });

        let body_text = step.body.template().unwrap_or("").to_owned();
        let body = text_input(cx, "{\n  \"key\": \"{{value}}\"\n}", body_text);
        body.update(cx, |input, cx| input.set_multiline(true, 10, cx));
        bind_step_text(&body, &weak, cx, index, |step, text| {
            step.body = match step.body {
                Body::None => Body::None,
                Body::Json { .. } => Body::Json {
                    template: text.to_owned(),
                },
                Body::Text { .. } => Body::Text {
                    template: text.to_owned(),
                },
            };
        });

        let headers = step
            .headers
            .iter()
            .enumerate()
            .map(|(row, key_value)| {
                build_key_value_row(cx, &weak, index, KeyValueKind::Header, row, key_value)
            })
            .collect();
        let query = step
            .query
            .iter()
            .enumerate()
            .map(|(row, key_value)| {
                build_key_value_row(cx, &weak, index, KeyValueKind::Query, row, key_value)
            })
            .collect();
        let assertions = step
            .assertions
            .iter()
            .enumerate()
            .map(|(row, assertion)| build_assertion_row(cx, &weak, index, row, assertion))
            .collect();
        let extractions = step
            .extract
            .iter()
            .enumerate()
            .map(|(row, extraction)| build_extraction_row(cx, &weak, index, row, extraction))
            .collect();

        self.form = Some(StepForm {
            index,
            name,
            url,
            timeout,
            expected_status,
            method,
            think_a,
            think_b,
            body,
            headers,
            query,
            assertions,
            extractions,
        });
    }

    fn rebuild_variables(&mut self, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        self.variables = self
            .scenario
            .variables
            .iter()
            .enumerate()
            .map(|(index, variable)| {
                let key = text_input(cx, "name", &variable.key);
                bind_variable(&key, &weak, cx, index, true);
                let value = text_input(cx, "value", &variable.value);
                bind_variable(&value, &weak, cx, index, false);
                VariableRow { key, value }
            })
            .collect();
    }

    // ---------------------------------------------------------------- actions

    fn select_step(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.selected_step != index {
            self.selected_step = index;
            self.tab = StepTab::Headers;
            self.response = None;
            self.test_error = None;
            self.rebuild_form(cx);
            cx.notify();
        }
    }

    fn set_tab(&mut self, tab: StepTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    fn add_step(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let index = self.scenario.steps.len();
        self.scenario.steps.push(Step::blank(index));
        self.selected_step = index;
        self.tab = StepTab::Headers;
        self.response = None;
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn insert_step_after(
        &mut self,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.scenario.steps.len() {
            return;
        }
        let mut step = Step::blank(index + 1);
        step.name = format!("Request {}", self.scenario.steps.len() + 1);
        self.scenario.steps.insert(index + 1, step);
        self.selected_step = index + 1;
        self.response = None;
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn duplicate_step(
        &mut self,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(step) = self.scenario.steps.get(index).cloned() else {
            return;
        };
        if index >= self.scenario.steps.len() {
            return;
        }
        let mut duplicate = step;
        duplicate.id = Id::now_v7();
        duplicate.name = format!("{} copy", duplicate.name);
        self.scenario.steps.insert(index + 1, duplicate);
        self.selected_step = index + 1;
        self.response = None;
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn delete_step(
        &mut self,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scenario.steps.len() <= 1 || index >= self.scenario.steps.len() {
            return;
        }
        self.scenario.steps.remove(index);
        if self.selected_step >= self.scenario.steps.len() {
            self.selected_step = self.scenario.steps.len() - 1;
        } else if self.selected_step > index {
            self.selected_step -= 1;
        }
        self.response = None;
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn move_step(
        &mut self,
        index: usize,
        delta: isize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = index as isize + delta;
        if target < 0 || target as usize >= self.scenario.steps.len() {
            return;
        }
        let target = target as usize;
        self.scenario.steps.swap(index, target);
        self.selected_step = target;
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn set_body_kind(&mut self, kind: BodyKind, cx: &mut Context<Self>) {
        let Some(step) = self.scenario.steps.get_mut(self.selected_step) else {
            return;
        };
        let template = step.body.template().unwrap_or("").to_owned();
        step.body = match kind {
            BodyKind::None => Body::None,
            BodyKind::Json => Body::Json { template },
            BodyKind::Text => Body::Text { template },
        };
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn set_think_kind(&mut self, kind: ThinkKind, cx: &mut Context<Self>) {
        let Some(step) = self.scenario.steps.get_mut(self.selected_step) else {
            return;
        };
        let (fixed, min, max) = match step.think_time {
            ThinkTime::None => (0, 0, 0),
            ThinkTime::Fixed { ms } => (ms, 0, 0),
            ThinkTime::Range { min_ms, max_ms } => (0, min_ms, max_ms),
        };
        step.think_time = match kind {
            ThinkKind::None => ThinkTime::None,
            ThinkKind::Fixed => ThinkTime::Fixed { ms: fixed },
            ThinkKind::Range => ThinkTime::Range {
                min_ms: if min == 0 { 10 } else { min },
                max_ms: if max == 0 { 50 } else { max },
            },
        };
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn format_json(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        let text = form.body.read(cx).content().to_owned();
        let formatted = match serde_json::from_str::<Value>(&text) {
            Ok(value) => serde_json::to_string_pretty(&value).unwrap_or(text),
            Err(error) => {
                self.test_error = Some(format!("JSON is not valid: {error}").into());
                cx.notify();
                return;
            }
        };
        if let Some(step) = self.scenario.steps.get_mut(self.selected_step) {
            step.body = Body::Json {
                template: formatted.clone(),
            };
        }
        form.body
            .update(cx, |input, cx| input.set_content(formatted, cx));
        self.mark_dirty();
        self.test_error = None;
        cx.notify();
    }

    fn add_header(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(step) = self.scenario.steps.get_mut(self.selected_step) {
            step.headers.push(KeyValue::new("", ""));
        }
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn add_query(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(step) = self.scenario.steps.get_mut(self.selected_step) {
            step.query.push(KeyValue::new("", ""));
        }
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn remove_key_value(
        &mut self,
        kind: KeyValueKind,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(step) = self.scenario.steps.get_mut(self.selected_step) {
            match kind {
                KeyValueKind::Header if index < step.headers.len() => {
                    step.headers.remove(index);
                }
                KeyValueKind::Query if index < step.query.len() => {
                    step.query.remove(index);
                }
                _ => return,
            }
        }
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn add_assertion(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let choice = self.assertion_kind.read(cx).selected();
        let assertion = match choice {
            1 => Assertion::BodyContains {
                value: String::new(),
            },
            2 => Assertion::BodyRegex {
                pattern: String::new(),
            },
            3 => Assertion::JsonPathEquals {
                path: "$.".to_owned(),
                value: Value::String(String::new()),
            },
            4 => Assertion::MaxDurationMs { value: 1_000 },
            5 => Assertion::MaxSizeBytes { value: 1_024 },
            _ => Assertion::HeaderEquals {
                name: "content-type".to_owned(),
                value: "application/json".to_owned(),
            },
        };
        if let Some(step) = self.scenario.steps.get_mut(self.selected_step) {
            step.assertions.push(assertion);
        }
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn remove_assertion(
        &mut self,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(step) = self.scenario.steps.get_mut(self.selected_step)
            && index < step.assertions.len()
        {
            step.assertions.remove(index);
        } else {
            return;
        }
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn add_extraction(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let choice = self.extraction_kind.read(cx).selected();
        let extraction = match choice {
            1 => Extraction::Regex {
                variable: "value".to_owned(),
                pattern: String::new(),
                group: 1,
            },
            2 => Extraction::Header {
                variable: "value".to_owned(),
                name: "content-type".to_owned(),
            },
            3 => Extraction::Status {
                variable: "status".to_owned(),
            },
            _ => Extraction::JsonPath {
                variable: "value".to_owned(),
                path: "$.".to_owned(),
            },
        };
        if let Some(step) = self.scenario.steps.get_mut(self.selected_step) {
            step.extract.push(extraction);
        }
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn remove_extraction(
        &mut self,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(step) = self.scenario.steps.get_mut(self.selected_step)
            && index < step.extract.len()
        {
            step.extract.remove(index);
        } else {
            return;
        }
        self.mark_dirty();
        self.rebuild_form(cx);
        cx.notify();
    }

    fn add_variable(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.scenario.variables.push(ScenarioVariable::new("", ""));
        self.mark_dirty();
        self.rebuild_variables(cx);
        cx.notify();
    }

    fn remove_variable(
        &mut self,
        index: usize,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.scenario.variables.len() {
            return;
        }
        self.scenario.variables.remove(index);
        self.mark_dirty();
        self.rebuild_variables(cx);
        cx.notify();
    }

    fn save(&mut self, _: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let mut scenario = self.scenario.clone();
        scenario.normalize();
        self.saving = true;
        self.save_error = None;
        self.test_error = None;
        cx.notify();

        let backend = self.backend.clone();
        let task = cx
            .background_executor()
            .spawn(async move { backend.save_scenario(scenario).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match result {
                    Ok(saved) => {
                        this.scenario = saved.clone();
                        this.dirty = false;
                        this.name
                            .update(cx, |input, cx| input.set_content(saved.name.clone(), cx));
                        this.description.update(cx, |input, cx| {
                            input.set_content(saved.description.clone(), cx)
                        });
                        this.rebuild_form(cx);
                        this.rebuild_variables(cx);
                        if let Some(callback) = this.on_saved.as_ref() {
                            callback(saved, cx);
                        }
                        this.notice = Some("Scenario saved".into());
                        let weak = cx.entity().downgrade();
                        cx.spawn(async move |_this, cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(2_500))
                                .await;
                            let _ = weak.update(cx, |this, cx| {
                                this.notice = None;
                                cx.notify();
                            });
                        })
                        .detach();
                    }
                    Err(error) => {
                        this.save_error = Some(error.message.into());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn request_close(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.dirty {
            self.confirm_close = true;
            cx.notify();
        } else {
            self.finish_close(cx);
        }
    }

    fn finish_close(&mut self, cx: &mut Context<Self>) {
        self.confirm_close = false;
        if let Some(callback) = self.on_close.take() {
            callback(cx);
        }
    }

    fn cancel_close(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.confirm_close = false;
        cx.notify();
    }

    fn toggle_variables(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_variables = !self.show_variables;
        cx.notify();
    }

    fn close_variables(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.show_variables = false;
        cx.notify();
    }

    fn test_step(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.testing {
            return;
        }
        let Some(environment_id) = self.selected_environment else {
            self.test_error = Some("Select an environment before testing a step.".into());
            cx.notify();
            return;
        };
        let request = TestStepRequest {
            scenario: self.scenario.clone(),
            step_index: self.selected_step,
            environment_id,
            extra_variables: self.extra_variables.clone(),
        };
        self.testing = true;
        self.test_error = None;
        self.response = None;
        cx.notify();

        let backend = self.backend.clone();
        let task = cx
            .background_executor()
            .spawn(async move { backend.test_step(request).await });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.testing = false;
                match result {
                    Ok(response) => {
                        for extraction in &response.extractions {
                            if let Some(value) = extraction.value.clone() {
                                if let Some(existing) = this
                                    .extra_variables
                                    .iter_mut()
                                    .find(|variable| variable.key == extraction.variable)
                                {
                                    existing.value = value;
                                } else {
                                    this.extra_variables.push(ScenarioVariable::new(
                                        extraction.variable.clone(),
                                        value,
                                    ));
                                }
                            }
                        }
                        this.response_tab = ResponseTab::Body;
                        this.response = Some(response);
                    }
                    Err(error) => {
                        this.test_error = Some(error.message.into());
                        if !error.fields.is_empty() {
                            this.tab = StepTab::Headers;
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn set_response_tab(&mut self, tab: ResponseTab, cx: &mut Context<Self>) {
        self.response_tab = tab;
        cx.notify();
    }

    fn copy_response_body(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(response) = self.response.as_ref() else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(response.body.clone()));
        self.notice = Some("Response body copied".into());
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(2_000))
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.notice = None;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ----------------------------------------------------------------- views

    fn header(&self, errors: &[FieldError], cx: &mut Context<Self>) -> Div {
        let issue_count = errors.len();
        let validation = if issue_count == 0 {
            success_badge("Valid")
        } else {
            warning_badge(format!(
                "{issue_count} issue{}",
                if issue_count == 1 { "" } else { "s" }
            ))
        };
        let name_error = error_for(errors, "name");
        let environment_label = self
            .selected_environment()
            .map(|environment| environment.name.clone())
            .unwrap_or_else(|| "No environment".to_owned());
        let can_test = self.selected_environment.is_some() && !self.testing;

        div()
            .flex()
            .flex_col()
            .gap_3()
            .px(px(18.))
            .py(px(14.))
            .bg(theme::bg_surface())
            .border_b_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        button("← Scenarios", ButtonVariant::Ghost, ButtonSize::Sm)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::request_close)),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_muted())
                            .child(self.project.name.clone()),
                    )
                    .child(div().text_size(px(9.)).text_color(theme::text_muted()).child("·"))
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child(format!("rev {}", self.scenario.revision)),
                    )
                    .child(validation)
                    .when(self.dirty, |this| this.child(warning_badge("Unsaved")))
                    .when_some(self.notice.clone(), |this, notice| {
                        this.child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::success())
                                .child(notice),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child(format!("Test target · {environment_label}")),
                    )
                    .child(div().w(px(220.)).child(self.env_select.clone()))
                    .child(
                        button("Test step", ButtonVariant::Secondary, ButtonSize::Sm).when(
                            can_test,
                            |this| {
                                this.on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(Self::test_step),
                                )
                            },
                        ),
                    )
                    .child(
                        button("Save scenario", ButtonVariant::Primary, ButtonSize::Sm)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::save)),
                    ),
            )
            .when_some(self.save_error.clone(), |this, error| {
                this.child(banner(BannerKind::Danger, error))
            })
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w(px(380.))
                            .child(field_label("Scenario name", None))
                            .child(self.name.clone())
                            .when_some(name_error, |this, error| {
                                this.child(inline_error(error))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(field_label("Description", None))
                            .child(self.description.clone()),
                    ),
            )
            .child(div().text_size(px(10.)).text_color(theme::text_muted()).child(
                "Steps run in order. Use {{base_url}}, scenario variables, and values extracted by earlier steps.",
            ))
    }

    fn step_list(&self, errors: &[FieldError], cx: &mut Context<Self>) -> Div {
        let active_count = self
            .scenario
            .steps
            .iter()
            .filter(|step| !self.step_errors(errors, step).is_empty())
            .count();
        let list = self
            .scenario
            .steps
            .iter()
            .enumerate()
            .map(|(index, step)| self.step_card(index, step, errors, cx))
            .collect::<Vec<_>>();
        let variable_count = self.scenario.variables.len();
        let environment_variables = self
            .selected_environment()
            .map(|environment| environment.variables.len())
            .unwrap_or(0);

        div()
            .flex()
            .flex_col()
            .w(px(320.))
            .h_full()
            .flex_shrink_0()
            .bg(theme::bg_surface())
            .border_r_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(14.))
                    .py(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text_muted())
                                    .child("STEPS"),
                            )
                            .child(
                                div()
                                    .text_size(px(9.))
                                    .text_color(theme::text_muted())
                                    .child(format!(
                                        "{} step{} · {} with issues",
                                        self.scenario.steps.len(),
                                        if self.scenario.steps.len() == 1 {
                                            ""
                                        } else {
                                            "s"
                                        },
                                        active_count
                                    )),
                            ),
                    )
                    .child(
                        button("+ Add step", ButtonVariant::Secondary, ButtonSize::Sm)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::add_step)),
                    ),
            )
            .child(
                div()
                    .id("scenario-step-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(10.))
                    .pb(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .p(px(8.))
                            .mb(px(6.))
                            .rounded(px(6.))
                            .bg(theme::bg_app())
                            .border_1()
                            .border_color(theme::border_subtle())
                            .cursor(gpui::CursorStyle::PointingHand)
                            .hover(|style| style.bg(theme::bg_raised()))
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::toggle_variables))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::text_secondary())
                                            .child("Variables"),
                                    )
                                    .child(neutral_badge(format!("{variable_count}"))),
                            )
                            .child(
                                div()
                                    .text_size(px(9.))
                                    .text_color(theme::text_muted())
                                    .child(format!("{environment_variables} from environment")),
                            ),
                    )
                    .children(list)
                    .child(
                        div().pt(px(2.)).child(
                            row_action("+ Add step")
                                .on_mouse_down(MouseButton::Left, cx.listener(Self::add_step)),
                        ),
                    ),
            )
    }

    fn step_card(
        &self,
        index: usize,
        step: &Step,
        errors: &[FieldError],
        cx: &mut Context<Self>,
    ) -> Div {
        let selected = index == self.selected_step;
        let step_errors = self.step_errors(errors, step);
        let name = if step.name.trim().is_empty() {
            "Untitled step".to_owned()
        } else {
            step.name.clone()
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .p(px(8.))
            .mb(px(6.))
            .rounded(px(6.))
            .bg(if selected {
                theme::bg_raised()
            } else {
                theme::bg_app()
            })
            .border_1()
            .border_color(if selected {
                theme::accent()
            } else {
                theme::border_subtle()
            })
            .cursor(gpui::CursorStyle::PointingHand)
            .when(!selected, |this| {
                this.hover(|style| style.border_color(theme::border_default()))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.select_step(index, cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(method_badge(step.method))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_primary())
                            .child(name),
                    )
                    .when(!step_errors.is_empty(), |this| {
                        this.child(warning_badge(format!("{}", step_errors.len())))
                    }),
            )
            .child(
                div()
                    .text_size(px(9.))
                    .text_color(theme::text_muted())
                    .child(if step.url.trim().is_empty() {
                        "No URL".to_owned()
                    } else {
                        step.url.clone()
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(neutral_badge(format!(
                        "{} assertion{}",
                        step.assertion_count(),
                        if step.assertion_count() == 1 { "" } else { "s" }
                    )))
                    .child(neutral_badge(format!(
                        "{} extract{}",
                        step.extract.len(),
                        if step.extract.len() == 1 { "" } else { "s" }
                    ))),
            )
            .when(selected, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .pt(px(2.))
                        .child(row_action("↑").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, window, cx| {
                                cx.stop_propagation();
                                this.move_step(index, -1, event, window, cx)
                            }),
                        ))
                        .child(row_action("↓").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, window, cx| {
                                cx.stop_propagation();
                                this.move_step(index, 1, event, window, cx)
                            }),
                        ))
                        .child(row_action("Duplicate").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, window, cx| {
                                cx.stop_propagation();
                                this.duplicate_step(index, event, window, cx)
                            }),
                        ))
                        .child(row_action("Insert after").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, window, cx| {
                                cx.stop_propagation();
                                this.insert_step_after(index, event, window, cx)
                            }),
                        ))
                        .when(self.scenario.steps.len() > 1, |this| {
                            this.child(row_action("Delete").on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event, window, cx| {
                                    cx.stop_propagation();
                                    this.delete_step(index, event, window, cx)
                                }),
                            ))
                        }),
                )
            })
    }

    fn step_errors(&self, errors: &[FieldError], step: &Step) -> Vec<FieldError> {
        let Some(index) = self
            .scenario
            .steps
            .iter()
            .position(|candidate| candidate.id == step.id)
        else {
            return Vec::new();
        };
        let prefix = format!("steps[{index}]");
        let nested_prefix = format!("{prefix}.");
        errors
            .iter()
            .filter(|error| error.field == prefix || error.field.starts_with(&nested_prefix))
            .cloned()
            .collect()
    }

    fn inspector(&self, errors: &[FieldError], cx: &mut Context<Self>) -> AnyElement {
        let Some(form) = self.form.as_ref() else {
            return div()
                .flex_1()
                .p(px(18.))
                .child(empty_state(
                    "No steps yet",
                    "Add a step to build the scenario.",
                ))
                .into_any_element();
        };
        let step_index = form.index;
        let step = &self.scenario.steps[step_index];
        let step_prefix = format!("steps[{step_index}]");
        let tab_errors = |tab: StepTab| -> usize {
            let prefix = match tab {
                StepTab::Headers => format!("{step_prefix}.headers"),
                StepTab::Query => format!("{step_prefix}.query"),
                StepTab::Body => format!("{step_prefix}.body"),
                StepTab::Assertions => format!("{step_prefix}.assert"),
                StepTab::Extract => format!("{step_prefix}.extract"),
            };
            errors
                .iter()
                .filter(|error| error.field.starts_with(&prefix))
                .count()
        };
        let tabs = StepTab::ALL
            .into_iter()
            .map(|tab| {
                let label = match tab {
                    StepTab::Headers => format!("Headers ({})", step.headers.len()),
                    StepTab::Query => format!("Query ({})", step.query.len()),
                    StepTab::Body => "Body".to_owned(),
                    StepTab::Assertions => format!("Assertions ({})", step.assertion_count()),
                    StepTab::Extract => format!("Extract ({})", step.extract.len()),
                };
                let issues = tab_errors(tab);
                tab_button(label, issues, self.tab == tab).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)),
                )
            })
            .collect::<Vec<_>>();

        let content = match self.tab {
            StepTab::Headers => self.headers_tab(form, errors, cx),
            StepTab::Query => self.query_tab(form, errors, cx),
            StepTab::Body => self.body_tab(form, step, errors, cx),
            StepTab::Assertions => self.assertions_tab(form, step, errors, cx),
            StepTab::Extract => self.extract_tab(form, step, errors, cx),
        };

        div()
            .id("scenario-inspector-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .p(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .max_w(px(940.))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .flex_1()
                                    .child(field_label("Step name", None))
                                    .child(form.name.clone())
                                    .when_some(
                                        error_for(
                                            errors,
                                            &format!("{step_prefix}.name"),
                                        ),
                                        |this, error| this.child(inline_error(error)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .w(px(118.))
                                    .child(field_label("Method", None))
                                    .child(form.method.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(field_label(
                                "URL",
                                Some("Relative to the environment base URL, or absolute. {{variables}} are allowed.".into()),
                            ))
                            .child(form.url.clone())
                            .when_some(
                                error_for(errors, &format!("{step_prefix}.url")),
                                |this, error| this.child(inline_error(error)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_3()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .w(px(140.))
                                    .child(field_label("Timeout (ms)", None))
                                    .child(form.timeout.clone())
                                    .when_some(
                                        error_for(
                                            errors,
                                            &format!("{step_prefix}.timeout_ms"),
                                        ),
                                        |this, error| this.child(inline_error(error)),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .w(px(190.))
                                    .child(field_label(
                                        "Expected status",
                                        Some("Comma-separated, e.g. 200, 204".into()),
                                    ))
                                    .child(form.expected_status.clone())
                                    .when_some(
                                        error_for(
                                            errors,
                                            &format!("{step_prefix}.expected_status"),
                                        ),
                                        |this, error| this.child(inline_error(error)),
                                    ),
                            )
                            .child(self.think_time_editor(form, step, errors, cx)),
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
                                    .gap_1()
                                    .border_b_1()
                                    .border_color(theme::border_subtle())
                                    .pb(px(6.))
                                    .children(tabs),
                            )
                            .child(content),
                    )
                    .child(self.tester_panel(cx)),
            )
            .into_any_element()
    }

    fn think_time_editor(
        &self,
        form: &StepForm,
        step: &Step,
        errors: &[FieldError],
        cx: &mut Context<Self>,
    ) -> Div {
        let prefix = format!("steps[{}]", form.index);
        let kind = think_kind(&step.think_time);
        let chips = [
            (ThinkKind::None, "None"),
            (ThinkKind::Fixed, "Fixed"),
            (ThinkKind::Range, "Range"),
        ]
        .into_iter()
        .map(|(value, label)| {
            let selected = value == kind;
            div()
                .h(px(26.))
                .px(px(8.))
                .rounded(px(4.))
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
                .text_size(px(10.))
                .text_color(if selected {
                    theme::accent()
                } else {
                    theme::text_secondary()
                })
                .cursor(gpui::CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.set_think_kind(value, cx)),
                )
                .child(label)
        });

        div()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .child(field_label("Think time", None))
            .child(div().flex().items_center().gap_1().children(chips))
            .when(kind == ThinkKind::Fixed, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(110.)).child(form.think_a.clone()))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::text_muted())
                                .child("ms before the next step"),
                        ),
                )
            })
            .when(kind == ThinkKind::Range, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(90.)).child(form.think_a.clone()))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::text_muted())
                                .child("to"),
                        )
                        .child(div().w(px(90.)).child(form.think_b.clone()))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::text_muted())
                                .child("ms"),
                        ),
                )
            })
            .when_some(
                error_for(errors, &format!("{prefix}.think_time.ms"))
                    .or_else(|| error_for(errors, &format!("{prefix}.think_time.min_ms")))
                    .or_else(|| error_for(errors, &format!("{prefix}.think_time.max_ms"))),
                |this, error| this.child(inline_error(error)),
            )
    }

    fn headers_tab(&self, form: &StepForm, errors: &[FieldError], cx: &mut Context<Self>) -> Div {
        let index = form.index;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(
                                "Header values may reference variables and environment secrets.",
                            ),
                    )
                    .child(
                        row_action("+ Add header")
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::add_header)),
                    ),
            )
            .when(form.headers.is_empty(), |this| {
                this.child(hint_panel(
                    "No headers",
                    "Add Accept, Content-Type, or an authorization variable.",
                ))
            })
            .children(form.headers.iter().enumerate().map(|(row, header)| {
                key_value_row(
                    header,
                    format!("steps[{index}].headers[{row}]"),
                    errors,
                    cx,
                    KeyValueKind::Header,
                    row,
                )
            }))
    }

    fn query_tab(&self, form: &StepForm, errors: &[FieldError], cx: &mut Context<Self>) -> Div {
        let index = form.index;
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(
                                "Query parameters are URL-encoded. Secrets are not allowed here.",
                            ),
                    )
                    .child(
                        row_action("+ Add parameter")
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::add_query)),
                    ),
            )
            .when(form.query.is_empty(), |this| {
                this.child(hint_panel(
                    "No query parameters",
                    "Add filters or paging values; they are appended to the URL.",
                ))
            })
            .children(form.query.iter().enumerate().map(|(row, parameter)| {
                key_value_row(
                    parameter,
                    format!("steps[{index}].query[{row}]"),
                    errors,
                    cx,
                    KeyValueKind::Query,
                    row,
                )
            }))
    }

    fn body_tab(
        &self,
        form: &StepForm,
        step: &Step,
        errors: &[FieldError],
        cx: &mut Context<Self>,
    ) -> Div {
        let prefix = format!("steps[{}].body.template", form.index);
        let kind = body_kind(&step.body);
        let chips = [
            (BodyKind::None, "None"),
            (BodyKind::Json, "JSON"),
            (BodyKind::Text, "Raw text"),
        ]
        .into_iter()
        .map(|(value, label)| {
            let selected = value == kind;
            div()
                .h(px(26.))
                .px(px(8.))
                .rounded(px(4.))
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
                .text_size(px(10.))
                .text_color(if selected {
                    theme::accent()
                } else {
                    theme::text_secondary()
                })
                .cursor(gpui::CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.set_body_kind(value, cx)),
                )
                .child(label)
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(field_label("Body type", None))
                    .children(chips)
                    .child(div().flex_1())
                    .when(kind != BodyKind::None, |this| {
                        let count = form.body.read(cx).content().chars().count();
                        this.child(
                            div()
                                .text_size(px(9.))
                                .text_color(theme::text_muted())
                                .child(format!("{count} characters")),
                        )
                    })
                    .when(kind == BodyKind::Json, |this| {
                        this.child(
                            row_action("Format JSON")
                                .on_mouse_down(MouseButton::Left, cx.listener(Self::format_json)),
                        )
                    }),
            )
            .when(kind == BodyKind::None, |this| {
                this.child(hint_panel(
                    "No request body",
                    "Choose JSON or raw text to send a payload. GET and HEAD requests never send a body.",
                ))
            })
            .when(kind != BodyKind::None, |this| {
                this.child(form.body.clone())
            })
            .when_some(error_for(errors, &prefix), |this, error| {
                this.child(inline_error(error))
            })
            .when_some(
                error_for(errors, &format!("steps[{}].body", form.index)),
                |this, error| this.child(inline_error(error)),
            )
    }

    fn assertions_tab(
        &self,
        form: &StepForm,
        step: &Step,
        errors: &[FieldError],
        cx: &mut Context<Self>,
    ) -> Div {
        let prefix = format!("steps[{}].assertions", form.index);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::text_muted())
                    .child(
                        "Assertions run during the single-request test and in load runs. The expected status is asserted first.",
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .p(px(8.))
                    .rounded(px(6.))
                    .bg(theme::bg_app())
                    .border_1()
                    .border_color(theme::border_subtle())
                    .child(neutral_badge("Status"))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .text_color(theme::text_secondary())
                            .child(format!(
                                "status in {}",
                                join_statuses(&step.expected_status)
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child("Edit in the request row"),
                    ),
            )
            .when(form.assertions.is_empty(), |this| {
                this.child(hint_panel(
                    "No extra assertions",
                    "Add a header, body, JSON path, duration, or size check.",
                ))
            })
            .children(
                form.assertions
                    .iter()
                    .enumerate()
                    .filter_map(|(row, _)| {
                        let assertion = step.assertions.get(row)?;
                        Some(self.assertion_row(
                            form,
                            row,
                            assertion,
                            &format!("{prefix}[{row}]"),
                            errors,
                            cx,
                        ))
                    })
                    .collect::<Vec<_>>(),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(190.)).child(self.assertion_kind.clone()))
                    .child(
                        button("Add assertion", ButtonVariant::Secondary, ButtonSize::Sm)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::add_assertion)),
                    ),
            )
    }

    fn assertion_row(
        &self,
        form: &StepForm,
        row: usize,
        assertion: &Assertion,
        prefix: &str,
        errors: &[FieldError],
        cx: &mut Context<Self>,
    ) -> Div {
        let _ = form;
        let body = match assertion {
            Assertion::StatusIn { values } => div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme::text_muted())
                        .child(format!("status in {}", join_statuses(values))),
                )
                .into_any_element(),
            Assertion::HeaderEquals { .. } | Assertion::BodyContains { .. } => {
                let row_ref =
                    match assertion {
                        Assertion::HeaderEquals { name, value } => {
                            let input = self.form.as_ref().and_then(|form| {
                                match form.assertions.get(row) {
                                    Some(AssertionRow::HeaderEquals { name, value }) => {
                                        Some((name.clone(), value.clone()))
                                    }
                                    _ => None,
                                }
                            });
                            match input {
                                Some((name_input, value_input)) => div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(div().w(px(190.)).child(name_input).when_some(
                                        error_for(errors, &format!("{prefix}.name")),
                                        |this, error| this.child(inline_error(error)),
                                    ))
                                    .child(div().flex_1().child(value_input).when_some(
                                        error_for(errors, &format!("{prefix}.value")),
                                        |this, error| this.child(inline_error(error)),
                                    )),
                                None => div().child(format!("{name} equals {value}")),
                            }
                        }
                        Assertion::BodyContains { value } => {
                            let input = self.form.as_ref().and_then(|form| {
                                match form.assertions.get(row) {
                                    Some(AssertionRow::BodyContains { value }) => {
                                        Some(value.clone())
                                    }
                                    _ => None,
                                }
                            });
                            match input {
                                Some(input) => div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(9.))
                                            .text_color(theme::text_muted())
                                            .child("Body must contain"),
                                    )
                                    .child(input)
                                    .when_some(
                                        error_for(errors, &format!("{prefix}.value")),
                                        |this, error| this.child(inline_error(error)),
                                    ),
                                None => div().child(value.clone()),
                            }
                        }
                        _ => div(),
                    };
                row_ref.into_any_element()
            }
            Assertion::BodyRegex { .. } => {
                let input = self
                    .form
                    .as_ref()
                    .and_then(|form| match form.assertions.get(row) {
                        Some(AssertionRow::BodyRegex { pattern }) => Some(pattern.clone()),
                        _ => None,
                    });
                match input {
                    Some(input) => div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(9.))
                                .text_color(theme::text_muted())
                                .child("Body matches regular expression"),
                        )
                        .child(input)
                        .when_some(
                            error_for(errors, &format!("{prefix}.pattern")),
                            |this, error| this.child(inline_error(error)),
                        )
                        .into_any_element(),
                    None => div().into_any_element(),
                }
            }
            Assertion::JsonPathEquals { .. } => {
                let input = self
                    .form
                    .as_ref()
                    .and_then(|form| match form.assertions.get(row) {
                        Some(AssertionRow::JsonPathEquals { path, value }) => {
                            Some((path.clone(), value.clone()))
                        }
                        _ => None,
                    });
                match input {
                    Some((path, value)) => div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(190.)).child(path).when_some(
                            error_for(errors, &format!("{prefix}.path")),
                            |this, error| this.child(inline_error(error)),
                        ))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::text_muted())
                                .child("equals"),
                        )
                        .child(div().flex_1().child(value).when_some(
                            error_for(errors, &format!("{prefix}.value")),
                            |this, error| this.child(inline_error(error)),
                        ))
                        .into_any_element(),
                    None => div().into_any_element(),
                }
            }
            Assertion::MaxDurationMs { .. } | Assertion::MaxSizeBytes { .. } => {
                let input = self
                    .form
                    .as_ref()
                    .and_then(|form| match form.assertions.get(row) {
                        Some(AssertionRow::MaxDuration { value })
                        | Some(AssertionRow::MaxSize { value }) => Some(value.clone()),
                        _ => None,
                    });
                match input {
                    Some(input) => div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(120.)).child(input))
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::text_muted())
                                .child(match assertion {
                                    Assertion::MaxDurationMs { .. } => "ms maximum",
                                    _ => "bytes maximum",
                                }),
                        )
                        .when_some(
                            error_for(errors, &format!("{prefix}.value")),
                            |this, error| this.child(inline_error(error)),
                        )
                        .into_any_element(),
                    None => div().into_any_element(),
                }
            }
        };

        div()
            .flex()
            .flex_col()
            .gap_1()
            .p(px(8.))
            .rounded(px(6.))
            .bg(theme::bg_app())
            .border_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(neutral_badge(assertion.kind_label()))
                    .child(div().flex_1())
                    .child(row_action("Remove").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.remove_assertion(row, event, window, cx)
                        }),
                    )),
            )
            .child(body)
    }

    fn extract_tab(
        &self,
        form: &StepForm,
        step: &Step,
        errors: &[FieldError],
        cx: &mut Context<Self>,
    ) -> Div {
        let prefix = format!("steps[{}].extract", form.index);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::text_muted())
                    .child("Extracted values are available to later steps in the same iteration. Single-request tests reuse values extracted earlier in this editor session."),
            )
            .when(step.extract.is_empty(), |this| {
                this.child(hint_panel(
                    "No extractions",
                    "Capture an id or token so the next step can reference it as {{name}}.",
                ))
            })
            .children(
                form.extractions
                    .iter()
                    .enumerate()
                    .filter_map(|(row, _)| {
                        let extraction = step.extract.get(row)?;
                        Some(self.extraction_row(
                            row,
                            extraction,
                            &format!("{prefix}[{row}]"),
                            errors,
                            cx,
                        ))
                    })
                    .collect::<Vec<_>>(),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(170.)).child(self.extraction_kind.clone()))
                    .child(
                        button("Add extraction", ButtonVariant::Secondary, ButtonSize::Sm)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::add_extraction)),
                    ),
            )
    }

    fn extraction_row(
        &self,
        row: usize,
        extraction: &Extraction,
        prefix: &str,
        errors: &[FieldError],
        cx: &mut Context<Self>,
    ) -> Div {
        let fields = match (
            extraction,
            self.form
                .as_ref()
                .and_then(|form| form.extractions.get(row)),
        ) {
            (Extraction::JsonPath { .. }, Some(ExtractionRow::JsonPath { variable, path })) => {
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w(px(170.))
                            .child(hint_text("Variable"))
                            .child(variable.clone())
                            .when_some(
                                error_for(errors, &format!("{prefix}.variable")),
                                |this, error| this.child(inline_error(error)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(hint_text("JSON path"))
                            .child(path.clone())
                            .when_some(
                                error_for(errors, &format!("{prefix}.expression")),
                                |this, error| this.child(inline_error(error)),
                            ),
                    )
            }
            (
                Extraction::Regex { .. },
                Some(ExtractionRow::Regex {
                    variable,
                    pattern,
                    group,
                }),
            ) => div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .w(px(150.))
                        .child(hint_text("Variable"))
                        .child(variable.clone())
                        .when_some(
                            error_for(errors, &format!("{prefix}.variable")),
                            |this, error| this.child(inline_error(error)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flex_1()
                        .child(hint_text("Regular expression"))
                        .child(pattern.clone())
                        .when_some(
                            error_for(errors, &format!("{prefix}.expression")),
                            |this, error| this.child(inline_error(error)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .w(px(70.))
                        .child(hint_text("Group"))
                        .child(group.clone())
                        .when_some(
                            error_for(errors, &format!("{prefix}.group")),
                            |this, error| this.child(inline_error(error)),
                        ),
                ),
            (Extraction::Header { .. }, Some(ExtractionRow::Header { variable, name })) => div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .w(px(170.))
                        .child(hint_text("Variable"))
                        .child(variable.clone())
                        .when_some(
                            error_for(errors, &format!("{prefix}.variable")),
                            |this, error| this.child(inline_error(error)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flex_1()
                        .child(hint_text("Response header name"))
                        .child(name.clone())
                        .when_some(
                            error_for(errors, &format!("{prefix}.expression")),
                            |this, error| this.child(inline_error(error)),
                        ),
                ),
            (Extraction::Status { .. }, Some(ExtractionRow::Status { variable })) => div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .w(px(170.))
                        .child(hint_text("Variable"))
                        .child(variable.clone())
                        .when_some(
                            error_for(errors, &format!("{prefix}.variable")),
                            |this, error| this.child(inline_error(error)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme::text_muted())
                        .child("The numeric HTTP status code."),
                ),
            _ => div(),
        };

        div()
            .flex()
            .flex_col()
            .gap_1()
            .p(px(8.))
            .rounded(px(6.))
            .bg(theme::bg_app())
            .border_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(neutral_badge(extraction.kind_label()))
                    .child(div().flex_1())
                    .child(row_action("Remove").on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event, window, cx| {
                            this.remove_extraction(row, event, window, cx)
                        }),
                    )),
            )
            .child(fields)
    }

    fn tester_panel(&self, cx: &mut Context<Self>) -> Div {
        let environment = self.selected_environment();
        let environment_label = environment
            .map(|environment| format!("{} · {}", environment.name, environment.kind.label()))
            .unwrap_or_else(|| "No environment selected".to_owned());
        let disabled = environment.is_none() || self.testing;
        let send = if self.testing {
            div()
                .flex()
                .items_center()
                .h(px(30.))
                .px(px(12.))
                .rounded(px(6.))
                .bg(theme::bg_raised())
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child("Sending…")
        } else if disabled {
            button("Send request", ButtonVariant::Primary, ButtonSize::Sm)
                .opacity(0.45)
                .cursor(gpui::CursorStyle::Arrow)
        } else {
            button("Send request", ButtonVariant::Primary, ButtonSize::Sm)
                .on_mouse_down(MouseButton::Left, cx.listener(Self::test_step))
        };

        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(send)
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(format!("against {environment_label}")),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::text_muted())
                            .child("Responses stay in memory; secrets are masked."),
                    ),
            )
            .when_some(self.test_error.clone(), |this, error| {
                this.child(banner(BannerKind::Danger, error))
            });

        if let Some(response) = self.response.as_ref() {
            panel = panel.child(self.response_panel(response, cx));
        }
        panel
    }

    fn response_panel(&self, response: &TestStepResponse, cx: &mut Context<Self>) -> Div {
        let status = response.status;
        let status_color = status_color(status);
        let request = &response.request;
        let headers = response
            .headers
            .iter()
            .map(|(name, value)| {
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .py(px(2.))
                    .child(
                        div()
                            .w(px(180.))
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(10.))
                            .text_color(theme::text_secondary())
                            .child(value.clone()),
                    )
            })
            .collect::<Vec<_>>();
        let response_body_lines = body_lines(&response.body);
        let response_tab = self.response_tab;
        let response_tabs = [
            (ResponseTab::Headers, "Headers"),
            (ResponseTab::Body, "Body"),
        ]
        .into_iter()
        .map(|(tab, label)| {
            let selected = response_tab == tab;
            div()
                .h(px(24.))
                .px(px(8.))
                .rounded(px(4.))
                .bg(if selected {
                    theme::bg_raised()
                } else {
                    theme::bg_app()
                })
                .text_size(px(10.))
                .text_color(if selected {
                    theme::text_primary()
                } else {
                    theme::text_muted()
                })
                .cursor(gpui::CursorStyle::PointingHand)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.set_response_tab(tab, cx)),
                )
                .child(label)
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .p(px(10.))
            .rounded(px(6.))
            .bg(theme::bg_surface())
            .border_1()
            .border_color(theme::border_subtle())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(neutral_badge(request.method.as_str()))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(10.))
                            .text_color(theme::text_secondary())
                            .child(request.url.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .h(px(22.))
                            .px(px(7.))
                            .rounded(px(4.))
                            .bg(theme::tint(status_color))
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(status_color)
                            .child(format!("{status}")),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child(format!(
                                "{} ms · {} · {}",
                                response.duration_ms,
                                format_bytes(response.size_bytes),
                                if response.truncated {
                                    "truncated at 256 KB"
                                } else {
                                    "complete body"
                                }
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p(px(8.))
                    .rounded(px(4.))
                    .bg(theme::bg_app())
                    .child(hint_text("Request sent"))
                    .when_some(request.body.clone(), |this, body| {
                        this.child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::text_muted())
                                .child(format!("Body · {} characters", body.chars().count())),
                        )
                        .child(
                            div()
                                .id("request-body-preview")
                                .max_h(px(140.))
                                .overflow_y_scroll()
                                .p(px(6.))
                                .rounded(px(4.))
                                .bg(theme::bg_sunken())
                                .children(body_lines(&body)),
                        )
                    })
                    .children(request.headers.iter().map(|(name, value)| {
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(170.))
                                    .text_size(px(9.))
                                    .text_color(theme::text_muted())
                                    .child(name.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(9.))
                                    .text_color(theme::text_secondary())
                                    .child(value.clone()),
                            )
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(response_tabs)
                    .child(div().flex_1())
                    .when(response_tab == ResponseTab::Body, |this| {
                        this.child(row_action("Copy body").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(Self::copy_response_body),
                        ))
                    }),
            )
            .when(response_tab == ResponseTab::Headers, |this| {
                this.child(div().flex().flex_col().gap_1().children(headers))
            })
            .when(response_tab == ResponseTab::Body, |this| {
                this.child(
                    div()
                        .id("response-body-scroll")
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .p(px(8.))
                        .rounded(px(4.))
                        .bg(theme::bg_sunken())
                        .children(response_body_lines),
                )
            })
            .when(response.truncated, |this| {
                this.child(warning_badge("Body truncated at 256 KB"))
            })
            .when(!response.assertions.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(hint_text("Assertions"))
                        .children(response.assertions.iter().map(|result| {
                            div()
                                .flex()
                                .items_start()
                                .gap_2()
                                .child(if result.passed {
                                    success_badge("Pass")
                                } else {
                                    warning_badge("Fail")
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(10.))
                                        .text_color(if result.passed {
                                            theme::text_secondary()
                                        } else {
                                            theme::danger()
                                        })
                                        .child(format!("{} — {}", result.label, result.detail)),
                                )
                        })),
                )
            })
            .when(!response.extractions.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(hint_text("Extracted values"))
                        .children(response.extractions.iter().map(|result| {
                            div()
                                .flex()
                                .items_start()
                                .gap_2()
                                .child(
                                    div()
                                        .w(px(170.))
                                        .text_size(px(10.))
                                        .text_color(theme::text_muted())
                                        .child(result.variable.clone()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(10.))
                                        .text_color(if result.error.is_some() {
                                            theme::danger()
                                        } else {
                                            theme::text_secondary()
                                        })
                                        .child(match (&result.value, &result.error) {
                                            (Some(value), _) => format!("= {value}"),
                                            (None, Some(error)) => format!("failed: {error}"),
                                            (None, None) => "not found".to_owned(),
                                        }),
                                )
                        })),
                )
            })
    }

    fn variables_overlay(&self, cx: &mut Context<Self>) -> Div {
        let environment = self.selected_environment();
        let errors = self.validation_errors();
        let environment_rows = environment
            .map(|environment| {
                environment
                    .variables
                    .iter()
                    .map(|variable| {
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .p(px(8.))
                            .rounded(px(6.))
                            .bg(theme::bg_app())
                            .border_1()
                            .border_color(theme::border_subtle())
                            .child(
                                div()
                                    .w(px(190.))
                                    .text_size(px(11.))
                                    .text_color(theme::text_secondary())
                                    .child(variable.key.clone()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(11.))
                                    .text_color(theme::text_muted())
                                    .child(if variable.secret {
                                        "•••••• · keychain".to_owned()
                                    } else {
                                        variable.value.clone().unwrap_or_default()
                                    }),
                            )
                            .child(neutral_badge("Environment"))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let body = modal_body()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::text_muted())
                            .child("Scenario variables are sent into the k6 script. Secret variables are not supported in the MVP; use an environment secret."),
                    )
                    .child(
                        row_action("+ Add variable")
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::add_variable)),
                    ),
            )
            .when(self.scenario.variables.is_empty(), |this| {
                this.child(hint_panel(
                    "No scenario variables",
                    "Add a name and value, then reference it as {{name}}.",
                ))
            })
            .children(self.variables.iter().enumerate().map(|(index, row)| {
                let prefix = format!("variables[{index}]");
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
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w(px(190.))
                            .child(row.key.clone())
                            .when_some(
                                error_for(&errors, &format!("{prefix}.key")),
                                |this, error| this.child(inline_error(error)),
                            ),
                    )
                    .child(div().flex_1().child(row.value.clone()))
                    .child(
                        row_action("Remove").on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event, window, cx| {
                                this.remove_variable(index, event, window, cx)
                            }),
                        ),
                    )
            }))
            .when(!environment_rows.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(field_label(
                            "Environment variables",
                            Some("Read-only here; edit them in the environment.".into()),
                        ))
                        .children(environment_rows),
                )
            })
            .when_some(error_for(&errors, "variables"), |this, error| {
                this.child(inline_error(error))
            });

        let footer = modal_footer().child(
            button("Close", ButtonVariant::Primary, ButtonSize::Sm)
                .on_mouse_down(MouseButton::Left, cx.listener(Self::close_variables)),
        );

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
            .on_mouse_down(MouseButton::Left, cx.listener(Self::close_variables))
            .child(
                div()
                    .on_mouse_down(
                        MouseButton::Left,
                        |_: &MouseDownEvent, _: &mut Window, cx: &mut App| {
                            cx.stop_propagation();
                        },
                    )
                    .child(
                        modal_panel(760.0)
                            .child(modal_header(
                                "Variables",
                                Some("Scenario values and the selected environment.".into()),
                            ))
                            .child(body)
                            .child(footer),
                    ),
            )
    }

    fn close_overlay(&self, cx: &mut Context<Self>) -> Div {
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
            .child(
                modal_panel(520.0)
                    .child(modal_header(
                        "Discard unsaved changes?",
                        Some("The scenario has not been saved.".into()),
                    ))
                    .child(modal_body().child(banner(
                        BannerKind::Warning,
                        "Leaving now discards the edits in this editor. Saved drafts are unaffected.",
                    )))
                    .child(
                        modal_footer()
                            .child(
                                button("Keep editing", ButtonVariant::Secondary, ButtonSize::Sm)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(Self::cancel_close),
                                    ),
                            )
                            .child(
                                button("Discard changes", ButtonVariant::Danger, ButtonSize::Sm)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| this.finish_close(cx)),
                                    ),
                            ),
                    ),
            )
    }
}

impl Render for ScenarioEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let errors = self.validation_errors();
        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg_app())
            .text_color(theme::text_primary())
            .child(self.header(&errors, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.step_list(&errors, cx))
                    .child(self.inspector(&errors, cx)),
            );
        if self.show_variables {
            root = root.child(self.variables_overlay(cx));
        }
        if self.confirm_close {
            root = root.child(self.close_overlay(cx));
        }
        root
    }
}

impl Focusable for ScenarioEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.read(cx).focus_handle(cx)
    }
}

// ------------------------------------------------------------------ helpers

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyValueKind {
    Header,
    Query,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodyKind {
    None,
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThinkKind {
    None,
    Fixed,
    Range,
}

fn body_kind(body: &Body) -> BodyKind {
    match body {
        Body::None => BodyKind::None,
        Body::Json { .. } => BodyKind::Json,
        Body::Text { .. } => BodyKind::Text,
    }
}

fn think_kind(think_time: &ThinkTime) -> ThinkKind {
    match think_time {
        ThinkTime::None => ThinkKind::None,
        ThinkTime::Fixed { .. } => ThinkKind::Fixed,
        ThinkTime::Range { .. } => ThinkKind::Range,
    }
}

fn parse_u64(text: &str) -> u64 {
    text.trim().parse().unwrap_or(0)
}

fn parse_statuses(text: &str) -> Vec<u16> {
    let statuses: Vec<u16> = text
        .split([',', ' ', ';'])
        .filter_map(|part| part.trim().parse().ok())
        .collect();
    if statuses.is_empty() && !text.trim().is_empty() {
        vec![0]
    } else {
        statuses
    }
}

fn join_statuses(values: &[u16]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        values
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn text_input(
    cx: &mut Context<ScenarioEditor>,
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

fn bind_text(
    input: &Entity<TextInput>,
    weak: &WeakEntity<ScenarioEditor>,
    cx: &mut Context<ScenarioEditor>,
    update: impl Fn(&mut ScenarioEditor, &str) + 'static,
) {
    let weak = weak.clone();
    input.update(cx, |input, _| {
        input.set_on_change(Some(Box::new(move |text, _window, cx| {
            let _ = weak.update(cx, |this, cx| {
                update(this, text);
                this.mark_dirty();
                cx.notify();
            });
        })));
    });
}

fn bind_step_text(
    input: &Entity<TextInput>,
    weak: &WeakEntity<ScenarioEditor>,
    cx: &mut Context<ScenarioEditor>,
    step_index: usize,
    update: impl Fn(&mut Step, &str) + 'static,
) {
    bind_text(input, weak, cx, move |this, text| {
        if let Some(step) = this.scenario.steps.get_mut(step_index) {
            update(step, text);
        }
    });
}

fn bind_variable(
    input: &Entity<TextInput>,
    weak: &WeakEntity<ScenarioEditor>,
    cx: &mut Context<ScenarioEditor>,
    index: usize,
    key: bool,
) {
    bind_text(input, weak, cx, move |this, text| {
        if let Some(variable) = this.scenario.variables.get_mut(index) {
            if key {
                variable.key = text.to_owned();
            } else {
                variable.value = text.to_owned();
            }
        }
    });
}

fn bind_env_select(
    select: &Entity<Select>,
    weak: &WeakEntity<ScenarioEditor>,
    cx: &mut Context<ScenarioEditor>,
) {
    let weak = weak.clone();
    select.update(cx, |select, _| {
        select.set_on_change(Some(Box::new(move |choice, _window, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.selected_environment = this
                    .environments
                    .get(choice)
                    .map(|environment| environment.id);
                this.test_error = None;
                this.response = None;
                cx.notify();
            });
        })));
    });
}

fn build_key_value_row(
    cx: &mut Context<ScenarioEditor>,
    weak: &WeakEntity<ScenarioEditor>,
    step_index: usize,
    kind: KeyValueKind,
    row: usize,
    key_value: &KeyValue,
) -> KeyValueRow {
    let key = text_input(cx, "Name", &key_value.key);
    let key_weak = weak.clone();
    bind_text(&key, &key_weak, cx, move |this, text| {
        if let Some(step) = this.scenario.steps.get_mut(step_index) {
            let rows = match kind {
                KeyValueKind::Header => &mut step.headers,
                KeyValueKind::Query => &mut step.query,
            };
            if let Some(row) = rows.get_mut(row) {
                row.key = text.to_owned();
            }
        }
    });
    let value = text_input(cx, "Value", &key_value.value);
    let value_weak = weak.clone();
    bind_text(&value, &value_weak, cx, move |this, text| {
        if let Some(step) = this.scenario.steps.get_mut(step_index) {
            let rows = match kind {
                KeyValueKind::Header => &mut step.headers,
                KeyValueKind::Query => &mut step.query,
            };
            if let Some(row) = rows.get_mut(row) {
                row.value = text.to_owned();
            }
        }
    });
    KeyValueRow { key, value }
}

fn build_assertion_row(
    cx: &mut Context<ScenarioEditor>,
    weak: &WeakEntity<ScenarioEditor>,
    step_index: usize,
    row: usize,
    assertion: &Assertion,
) -> AssertionRow {
    match assertion {
        Assertion::StatusIn { .. } => AssertionRow::BodyContains {
            value: text_input(cx, "", ""),
        },
        Assertion::HeaderEquals { name, value } => {
            let name_input = text_input(cx, "Header", name);
            let name_weak = weak.clone();
            bind_text(&name_input, &name_weak, cx, move |this, text| {
                if let Some(Assertion::HeaderEquals { name, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.assertions.get_mut(row))
                {
                    *name = text.to_owned();
                }
            });
            let value_input = text_input(cx, "Expected value", value);
            let value_weak = weak.clone();
            bind_text(&value_input, &value_weak, cx, move |this, text| {
                if let Some(Assertion::HeaderEquals { value, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.assertions.get_mut(row))
                {
                    *value = text.to_owned();
                }
            });
            AssertionRow::HeaderEquals {
                name: name_input,
                value: value_input,
            }
        }
        Assertion::BodyContains { value } => {
            let input = text_input(cx, "Text", value);
            let weak = weak.clone();
            bind_text(&input, &weak, cx, move |this, text| {
                if let Some(Assertion::BodyContains { value }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.assertions.get_mut(row))
                {
                    *value = text.to_owned();
                }
            });
            AssertionRow::BodyContains { value: input }
        }
        Assertion::BodyRegex { pattern } => {
            let input = text_input(cx, "Pattern", pattern);
            let weak = weak.clone();
            bind_text(&input, &weak, cx, move |this, text| {
                if let Some(Assertion::BodyRegex { pattern }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.assertions.get_mut(row))
                {
                    *pattern = text.to_owned();
                }
            });
            AssertionRow::BodyRegex { pattern: input }
        }
        Assertion::JsonPathEquals { path, value } => {
            let path_input = text_input(cx, "$.id", path);
            let path_weak = weak.clone();
            bind_text(&path_input, &path_weak, cx, move |this, text| {
                if let Some(Assertion::JsonPathEquals { path, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.assertions.get_mut(row))
                {
                    *path = text.to_owned();
                }
            });
            let value_input = text_input(cx, "Value", json_scalar_text(value));
            let value_weak = weak.clone();
            bind_text(&value_input, &value_weak, cx, move |this, text| {
                if let Some(Assertion::JsonPathEquals { value, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.assertions.get_mut(row))
                {
                    *value = parse_scalar_input(text);
                }
            });
            AssertionRow::JsonPathEquals {
                path: path_input,
                value: value_input,
            }
        }
        Assertion::MaxDurationMs { value } => {
            let input = text_input(cx, "1000", value.to_string());
            let weak = weak.clone();
            bind_text(&input, &weak, cx, move |this, text| {
                if let Some(Assertion::MaxDurationMs { value }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.assertions.get_mut(row))
                {
                    *value = parse_u64(text);
                }
            });
            AssertionRow::MaxDuration { value: input }
        }
        Assertion::MaxSizeBytes { value } => {
            let input = text_input(cx, "1024", value.to_string());
            let weak = weak.clone();
            bind_text(&input, &weak, cx, move |this, text| {
                if let Some(Assertion::MaxSizeBytes { value }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.assertions.get_mut(row))
                {
                    *value = parse_u64(text);
                }
            });
            AssertionRow::MaxSize { value: input }
        }
    }
}

fn build_extraction_row(
    cx: &mut Context<ScenarioEditor>,
    weak: &WeakEntity<ScenarioEditor>,
    step_index: usize,
    row: usize,
    extraction: &Extraction,
) -> ExtractionRow {
    match extraction {
        Extraction::JsonPath { variable, path } => {
            let variable_input = text_input(cx, "name", variable);
            let variable_weak = weak.clone();
            bind_text(&variable_input, &variable_weak, cx, move |this, text| {
                if let Some(Extraction::JsonPath { variable, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.extract.get_mut(row))
                {
                    *variable = text.to_owned();
                }
            });
            let path_input = text_input(cx, "$.id", path);
            let path_weak = weak.clone();
            bind_text(&path_input, &path_weak, cx, move |this, text| {
                if let Some(Extraction::JsonPath { path, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.extract.get_mut(row))
                {
                    *path = text.to_owned();
                }
            });
            ExtractionRow::JsonPath {
                variable: variable_input,
                path: path_input,
            }
        }
        Extraction::Regex {
            variable,
            pattern,
            group,
        } => {
            let variable_input = text_input(cx, "name", variable);
            let variable_weak = weak.clone();
            bind_text(&variable_input, &variable_weak, cx, move |this, text| {
                if let Some(Extraction::Regex { variable, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.extract.get_mut(row))
                {
                    *variable = text.to_owned();
                }
            });
            let pattern_input = text_input(cx, "Pattern", pattern);
            let pattern_weak = weak.clone();
            bind_text(&pattern_input, &pattern_weak, cx, move |this, text| {
                if let Some(Extraction::Regex { pattern, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.extract.get_mut(row))
                {
                    *pattern = text.to_owned();
                }
            });
            let group_input = text_input(cx, "1", group.to_string());
            let group_weak = weak.clone();
            bind_text(&group_input, &group_weak, cx, move |this, text| {
                if let Some(Extraction::Regex { group, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.extract.get_mut(row))
                {
                    *group = text.trim().parse().unwrap_or(0);
                }
            });
            ExtractionRow::Regex {
                variable: variable_input,
                pattern: pattern_input,
                group: group_input,
            }
        }
        Extraction::Header { variable, name } => {
            let variable_input = text_input(cx, "name", variable);
            let variable_weak = weak.clone();
            bind_text(&variable_input, &variable_weak, cx, move |this, text| {
                if let Some(Extraction::Header { variable, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.extract.get_mut(row))
                {
                    *variable = text.to_owned();
                }
            });
            let name_input = text_input(cx, "content-type", name);
            let name_weak = weak.clone();
            bind_text(&name_input, &name_weak, cx, move |this, text| {
                if let Some(Extraction::Header { name, .. }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.extract.get_mut(row))
                {
                    *name = text.to_owned();
                }
            });
            ExtractionRow::Header {
                variable: variable_input,
                name: name_input,
            }
        }
        Extraction::Status { variable } => {
            let variable_input = text_input(cx, "status", variable);
            let variable_weak = weak.clone();
            bind_text(&variable_input, &variable_weak, cx, move |this, text| {
                if let Some(Extraction::Status { variable }) = this
                    .scenario
                    .steps
                    .get_mut(step_index)
                    .and_then(|step| step.extract.get_mut(row))
                {
                    *variable = text.to_owned();
                }
            });
            ExtractionRow::Status {
                variable: variable_input,
            }
        }
    }
}

fn key_value_row(
    row_data: &KeyValueRow,
    prefix: String,
    errors: &[FieldError],
    cx: &mut Context<ScenarioEditor>,
    kind: KeyValueKind,
    row: usize,
) -> Div {
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
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .w(px(200.))
                .child(row_data.key.clone())
                .when_some(
                    error_for(errors, &format!("{prefix}.key")),
                    |this, error| this.child(inline_error(error)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .child(row_data.value.clone())
                .when_some(
                    error_for(errors, &format!("{prefix}.value")),
                    |this, error| this.child(inline_error(error)),
                ),
        )
        .child(row_action("Remove").on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event, window, cx| {
                this.remove_key_value(kind, row, event, window, cx)
            }),
        ))
}

fn tab_button(label: String, issues: usize, selected: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .h(px(28.))
        .px(px(9.))
        .rounded(px(4.))
        .bg(if selected {
            theme::bg_raised()
        } else {
            theme::bg_surface()
        })
        .text_size(px(11.))
        .font_weight(if selected {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(if selected {
            theme::text_primary()
        } else {
            theme::text_muted()
        })
        .cursor(gpui::CursorStyle::PointingHand)
        .hover(|style| style.text_color(theme::text_secondary()))
        .child(label)
        .when(issues > 0, |this| {
            this.child(warning_badge(format!("{issues}")))
        })
}

fn method_badge(method: Method) -> Div {
    let color = match method {
        Method::Get => theme::success(),
        Method::Post => theme::accent(),
        Method::Put => theme::info(),
        Method::Patch => theme::warning(),
        Method::Delete => theme::danger(),
        Method::Head | Method::Options => theme::text_muted(),
    };
    crate::components::badge(method.as_str(), color)
}

fn hint_panel(title: impl Into<SharedString>, text: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p(px(12.))
        .rounded(px(6.))
        .bg(theme::bg_app())
        .border_1()
        .border_color(theme::border_subtle())
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_secondary())
                .child(title.into()),
        )
        .child(
            div()
                .text_size(px(10.))
                .text_color(theme::text_muted())
                .child(text.into()),
        )
}

fn hint_text(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(9.))
        .text_color(theme::text_muted())
        .child(text.into())
}

fn inline_error(message: impl Into<SharedString>) -> Div {
    inline_message(message.into(), true)
}

fn error_for(errors: &[FieldError], field: &str) -> Option<SharedString> {
    let messages: Vec<&str> = errors
        .iter()
        .filter(|error| error.field == field)
        .map(|error| error.message.as_str())
        .collect();
    if messages.is_empty() {
        None
    } else {
        Some(messages.join(" ").into())
    }
}

fn status_color(status: u16) -> gpui::Rgba {
    match status {
        100..=199 => theme::info(),
        200..=299 => theme::success(),
        300..=399 => theme::accent(),
        400..=499 => theme::warning(),
        _ => theme::danger(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.1} KB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

fn body_lines(body: &str) -> Vec<Div> {
    let lines: Vec<&str> = body.lines().take(400).collect();
    if lines.is_empty() {
        return vec![div().child(" ")];
    }
    lines
        .into_iter()
        .map(|line| {
            div()
                .text_size(px(10.))
                .text_color(theme::text_secondary())
                .child(if line.is_empty() {
                    " ".to_owned()
                } else {
                    line.to_owned()
                })
        })
        .collect()
}
