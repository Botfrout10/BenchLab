# BenchLab — API Performance Workbench

**Implementation specification**

| | |
|---|---|
| Spec version | 1.0 |
| Date | 2026-10-20 |
| Status | MVP implementation-ready |
| Product | BenchLab |
| Primary acceptance target | Windows 10/11 x64 |
| Secondary target | Linux (Wayland/X11), macOS best-effort |
| UI | GPUI 0.2.x |
| Load engine | k6 2.x behind a Rust engine abstraction |
| Storage | SQLite (WAL) + files on disk |
| MVP | Brainstorm Phase 1 |
| Deferred | AI reports, schedules, export, OpenAPI import, CI integration |

---

## Contents

1. [Product definition](#1-product-definition)
2. [Personas and jobs](#2-personas-and-jobs)
3. [Glossary](#3-glossary)
4. [Scope](#4-scope)
5. [Core journeys](#5-core-journeys)
6. [Functional requirements](#6-functional-requirements)
7. [Information architecture and screens](#7-information-architecture-and-screens)
8. [Visual system](#8-visual-system)
9. [Architecture](#9-architecture)
10. [Domain model](#10-domain-model)
11. [Persistence](#11-persistence)
12. [Engine integration](#12-engine-integration)
13. [Metrics and statistics](#13-metrics-and-statistics)
14. [Offline findings](#14-offline-findings)
15. [AI analysis (Phase 2)](#15-ai-analysis-phase-2)
16. [Automation (Phase 2 and 3)](#16-automation-phase-2-and-3)
17. [Testing](#17-testing)
18. [Performance budgets](#18-performance-budgets)
19. [Security and privacy](#19-security-and-privacy)
20. [Build, run, package](#20-build-run-package)
21. [Implementation plan](#21-implementation-plan)
22. [Risks](#22-risks)
23. [Open questions](#23-open-questions)
- [Appendix A: Example run snapshot](#appendix-a-example-run-snapshot)
- [Appendix B: Example generated k6 script](#appendix-b-example-generated-k6-script)
- [Appendix C: Metric mapping and formulas](#appendix-c-metric-mapping-and-formulas)
- [Appendix D: Keyboard shortcuts](#appendix-d-keyboard-shortcuts)
- [Appendix E: Acceptance checklist](#appendix-e-acceptance-checklist)

---

## 1. Product definition

### 1.1 Problem

API performance work is fragmented across ad-hoc scripts, load-testing CLIs, spreadsheets, dashboards, and memory. Three failures recur:

1. **Setup is repetitive.** Every test requires rebuilding requests, auth, data, and load settings.
2. **Results are not reproducible.** The script, environment, load profile, and engine version are rarely captured together.
3. **Results are not understandable.** A percentile table does not explain what changed, whether the result is trustworthy, or what to inspect next.

### 1.2 Product thesis

BenchLab is a native desktop **experiment workbench**:

> Define what you are testing, configure how load is generated, specify what success means, run it, and analyze the evidence.

An **experiment** combines a scenario, a load profile, an environment, and pass/fail objectives. A **run** is an immutable snapshot of that experiment plus measured results. Everything else — charts, comparisons, findings — is derived from runs.

### 1.3 Principles

1. **Reproducible by default.** Every run stores the exact configuration, engine version, environment class, and measurement window. Raw data is retained.
2. **Evidence over adjectives.** Numbers carry sample counts and units. Findings cite evidence. No unsupported causal claims.
3. **The engine is transparent.** The generated k6 script, CLI arguments, raw JSONL, and logs are inspectable.
4. **Safety is a feature.** Caps, preflight checks, host pinning, production confirmation, and a working stop control are part of the product, not an afterthought.
5. **The UI never generates load.** Rendering and load generation are isolated. The UI observes an event stream.
6. **Local-first.** No account, no telemetry, no cloud requirement. AI is opt-in in Phase 2.
7. **No fake precision.** Show sample counts, warm-up windows, and inconclusive verdicts. Do not present four significant digits from forty samples.
8. **One primary action per screen.** Dense, calm, instrument-panel UI. No dashboard card walls.

### 1.4 Non-goals

- Not a general-purpose API client or collection sync tool.
- Not a k6 script IDE. Advanced users can inspect the generated script, but the product configures tests through structured forms.
- Not a monitoring platform. There is no always-on production probing in the MVP.
- Not a distributed load generator in the MVP.
- Not a cloud service or team collaboration product.
- Not a target-side APM. Target CPU/memory telemetry is Phase 3; only load-generator resource usage is shown in the MVP.

---

## 2. Personas and jobs

| Persona | Context | Job to be done |
|---|---|---|
| Backend developer | Owns one or two services, tests locally or in staging | "Tell me whether this endpoint meets its latency budget before I ship." |
| Platform / SRE engineer | Maintains several services and environments | "Give me a repeatable capacity test and a comparison against last week's baseline." |
| Indie builder | Runs a small service on modest hardware | "Find the point where my server stops keeping up, without learning a load-testing DSL." |
| Reviewer | Reads a result, did not run it | "Show me what was tested, whether it passed, and what evidence supports the conclusion." |

Primary job: go from "I want to know whether this API handles 200 concurrent users" to a reproducible run with a clear verdict, meaningful charts, and a report, in under ten minutes.

---

## 3. Glossary

| Term | Meaning |
|---|---|
| Project | Top-level container for environments, scenarios, profiles, experiments, and runs. |
| Environment | Target base URL, auth, variables, TLS settings, safety caps, and an environment class. |
| Scenario | Ordered HTTP steps with assertions, extractions, and think time. |
| Step | One HTTP request definition inside a scenario. |
| Load profile | Executor, VU or arrival-rate settings, stages, warm-up, and graceful stop. |
| Objective | A measurable pass/fail criterion, for example p95 ≤ 250 ms. |
| Experiment | Saved combination of scenario, load profile, environment, objectives, baseline, and safety. |
| Run | One execution of an experiment, with an immutable configuration snapshot and artifacts. |
| Status | Lifecycle state of a run: preflight, starting, running, stopping, finalizing, completed, failed, aborted, interrupted. |
| Verdict | Outcome of objectives: passed, failed, inconclusive, aborted, error. |
| Measurement window | The part of a run after warm-up used for objectives and summary statistics. |
| Bucket | One-second aggregate of samples used for charts and persistence. |
| Baseline | A previous run of the same experiment used for comparison. |
| Finding | A deterministic, evidence-backed observation generated from run data. |
| Engine | Load-generation implementation. MVP: k6. |
| Artifact | File produced by a run: script, raw JSONL, summary, log, snapshot. |
| Revision | Monotonic integer incremented when a scenario or profile is saved. |
| Snapshot | Full configuration stored with a run, with secrets redacted. |

---

## 4. Scope

### 4.1 MVP (Phase 1)

**In scope**

- Multi-project workspace with project switcher, archive, and delete.
- Environments with base URL, class (local/staging/production), variables, secrets, auth, TLS, host pinning, and safety caps.
- Flat HTTP scenarios: ordered steps, method, URL, headers, query, body, timeout, think time, assertions, and variable extraction.
- Single-request testing from the scenario editor.
- Reusable load profiles with smoke, load, stress, spike, and soak presets plus custom executor settings.
- Objectives for latency percentiles, mean/min/max, throughput, HTTP error rate, combined error rate, assertion failure rate, timeout rate, and connection failure rate.
- Experiments with scenario, profile, environment, objectives, baseline, and notes.
- Preflight checks and production confirmation.
- k6 execution with live metrics, cancellation, graceful stop, and force stop.
- Run history with filters and deletion.
- Run report with verdict, objectives, latency over time, throughput, error breakdown, latency histogram, per-step table, findings, and configuration snapshot.
- Comparison of two to four runs from the same experiment, with overlay charts and deltas.
- Deterministic offline findings. No LLM.
- Engine detection and setup guidance.
- Bundled demo project and local mock target.
- Settings for engine path, data directory, appearance, and retention/disk usage.
- Windows 10/11 x64 acceptance; Linux build supported.

**Out of scope for MVP**

- AI-generated reports and provider settings.
- Schedules, recurring tests, suites, notifications, and CI integration.
- HTML/Markdown export.
- OpenAPI import.
- Per-step objectives (per-step metrics are shown, but objectives are run-level).
- Distributed or remote load generators.
- Target-side CPU/memory/telemetry integrations.
- CSV/data-file parameterization.
- gRPC, WebSocket, HTTP/3, and non-HTTP protocols.
- Team sync, cloud storage, accounts, and telemetry.
- Light theme. Dark theme only.
- macOS acceptance testing.

### 4.2 Phase 2

- AI-assisted analysis through an OpenAI-compatible endpoint, with structured evidence and redaction.
- HTML and Markdown report export with inline SVG charts.
- Local schedules with cron, timezone, missed-run policy, and desktop notifications.
- Suites: ordered experiments, stop on failure, baseline promotion.
- Headless CLI and CI integration with machine-readable results and exit codes.
- OpenAPI import.
- Per-step objectives.
- Automatic retention policies.
- Light theme.
- More chart interactions: brush zoom, annotations, crosshair synchronization.

### 4.3 Phase 3

- Native Rust load engine behind the same engine trait.
- Remote and distributed generators.
- Server telemetry integrations (Prometheus and similar).
- Regression gates and deployment hooks.
- Plugins, custom metrics, and scenario templates.
- Additional protocols.

---

## 5. Core journeys

### 5.1 First launch to first result

1. App opens to **Welcome**. Engine strip shows whether k6 was found.
2. User chooses **Try the demo** or **Create project**.
3. Demo creates a project, a three-step scenario against the local mock target, a load profile, an experiment, and a ready-to-run configuration.
4. User opens the experiment and clicks **Run**.
5. Preflight runs: engine, target, scenario, caps, disk, authorization.
6. Live run opens. Metrics appear within two seconds.
7. Run completes. Report shows verdict, objectives, charts, findings, and configuration.
8. User can compare the demo run with a second run or inspect the raw artifact.

Target: under 60 seconds from first launch to a completed smoke run, with no external network.

### 5.2 Build an experiment

1. Select or create a project.
2. Select an environment, or create one with base URL and auth.
3. Create a scenario: add steps, headers, body, assertions, extraction, think time.
4. Test individual steps against the target.
5. Choose a load profile preset or configure a custom executor.
6. Set objectives. Defaults: p95 latency, p99 latency, HTTP error rate. Throughput is optional because it depends on the target's expected capacity.
7. Save the experiment and select a baseline if one exists.
8. Run preflight. Fix blocking issues.
9. Start the run.

### 5.3 Observe a run

1. Live view shows elapsed time, total duration, RPS, p95, error rate, active VUs, and generator CPU/memory.
2. Charts update once per second from one-second buckets. Readouts update at up to 10 Hz from the latest snapshot.
3. Objectives show live progress: pending, passing, failing, insufficient data.
4. Errors stream in the right column, grouped by class and status.
5. **Stop** performs a graceful stop through k6's REST API. Force stop is a secondary action in the overflow menu.
6. On completion, the app transitions to the report and shows a toast. The run remains in history.

### 5.4 Compare and understand

1. Report shows the verdict and failed objectives first, not a generic metric grid.
2. Charts show latency with the objective line, throughput, errors, and distribution.
3. Findings identify objective breaches, error bursts, step outliers, tail-latency divergence, and warm-up effects, each with evidence.
4. Compare selects two to four runs, warns about incompatible configurations, overlays latency and throughput, and shows deltas with sample counts.
5. User can set a run as the experiment baseline.

### 5.5 Failure and recovery

| Failure | Behavior |
|---|---|
| k6 missing or unsupported | Preflight blocks with install instructions and a path picker. No process spawned. |
| Target unreachable | Preflight blocks with DNS/TCP/TLS/HTTP detail. |
| Secret missing | Preflight blocks and names the missing key, never the value. |
| Cap exceeded | Validation blocks start. Caps are environment-owned and cannot be overridden per run. |
| Production environment | Typed confirmation and authorization checkbox before start. |
| User stop | Graceful REST stop, wait for graceful period, then force kill. Verdict aborted. |
| Engine crash | Status failed, stderr excerpt shown, artifacts retained. |
| App crash during run | On next launch, the orphaned k6 process is stopped and the run is recovered from raw data as aborted/interrupted. |
| Disk low | Preflight warns or blocks; watchdog stops the run if free space falls below 200 MB. |
| Database locked/corrupt | App shows a blocking error with the database path and recovery guidance; no silent reset. |

---

## 6. Functional requirements

Requirement keywords: **MUST** is required for MVP; **SHOULD** is expected unless it blocks delivery; **MAY** is optional.

### 6.1 Projects

- **FR-PROJ-01** The app MUST create a project with a trimmed name of 1–80 characters, optional description, and an accent color from a fixed palette.
- **FR-PROJ-02** Project names MUST be unique case-insensitively within the workspace.
- **FR-PROJ-03** The app MUST list, search, open, rename, archive, and delete projects.
- **FR-PROJ-04** Deleting a project MUST require typing the project name and MUST delete its environments, scenarios, profiles, experiments, runs, and artifacts.
- **FR-PROJ-05** Archiving MUST hide the project from the default list without deleting data.
- **FR-PROJ-06** The app MUST remember the last opened project and route across restarts.
- **FR-PROJ-07** Project overview MUST show recent runs, current experiment list, and a p95 trend derived from completed runs. It MUST NOT show invented statistics when no runs exist.

### 6.2 Environments

- **FR-ENV-01** The app MUST create, edit, duplicate, archive, and delete environments inside a project.
- **FR-ENV-02** An environment MUST have a name, class (`local`, `staging`, `production`), base URL, variables, auth, TLS settings, and safety caps.
- **FR-ENV-03** Base URLs MUST be absolute `http` or `https` URLs without embedded credentials. Invalid URLs block save.
- **FR-ENV-04** Variables MUST support non-secret values and secret values. Secret values MUST be stored in the OS keychain, never in SQLite, snapshots, logs, scripts, or exports.
- **FR-ENV-05** Secrets MUST be resolvable at run time. Missing secrets MUST fail preflight with the key name and a fix action.
- **FR-ENV-06** Auth MUST support none, bearer token, basic username/password, and custom header. Query-string API keys are out of MVP because they would leak into k6 URL tags.
- **FR-ENV-07** Auth values MUST be passed to k6 through child-process environment variables, never through the command line or generated script.
- **FR-ENV-08** TLS verification MUST default to on. Turning it off MUST be per environment and MUST show a persistent warning on the environment and in every run using it.
- **FR-ENV-09** Environments MUST define safety caps: maximum VUs, maximum requests per second (optional), maximum duration, and whether confirmation is required.
- **FR-ENV-10** Production environments MUST require typed confirmation of the environment name plus an authorization checkbox for every run.
- **FR-ENV-11** Host pinning MUST be on by default: every resolved request URL must use the base URL host, unless the environment explicitly enables external hosts and lists them.
- **FR-ENV-12** A test request action MUST verify that variables and auth resolve and that the target responds. It sends one request and does not count toward run metrics.
- **FR-ENV-13** Environment revisions MUST increment on save. Runs snapshot the environment configuration with secret values replaced by fingerprints.

### 6.3 Scenarios and steps

- **FR-SCN-01** The app MUST create, edit, duplicate, archive, and delete scenarios.
- **FR-SCN-02** A scenario MUST contain at least one step and MAY contain variables.
- **FR-SCN-03** Steps MUST support method, URL, headers, query parameters, body, timeout, and think time.
- **FR-SCN-04** Supported methods: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS.
- **FR-SCN-05** Supported bodies: none, JSON, and raw text. Form and file bodies are Phase 2.
- **FR-SCN-06** Steps MUST be addable, duplicable, deletable, and reorderable with keyboard-accessible up/down controls.
- **FR-SCN-07** Headers and query parameters MUST be editable as key/value rows with add/remove.
- **FR-SCN-08** Header names and values MUST reject CR/LF injection.
- **FR-SCN-09** Assertions MUST support: status in list, header equals, body contains, body regex, JSON path equals (scalar), max duration, and response size limit.
- **FR-SCN-10** Extractions MUST support: JSON path (scalar), regex capture group, response header, and status code. Extracted values are available to later steps in the same iteration.
- **FR-SCN-11** Think time MUST support none, fixed milliseconds, and random range.
- **FR-SCN-12** Variable syntax MUST be `{{name}}`. Names MUST match `[A-Za-z_][A-Za-z0-9_]*` and MUST NOT shadow `base_url`, `run_id`, or names beginning with `__`.
- **FR-SCN-13** Secret variables MUST NOT be allowed in URLs or query strings. They MAY be used in headers and bodies.
- **FR-SCN-14** Saving MUST validate all static fields and report errors inline. A scenario with validation errors MUST NOT be runnable.
- **FR-SCN-15** The editor MUST provide a single-request test for any step, resolving the selected environment, variables, and auth. Secrets MUST be masked in the displayed request and response.
- **FR-SCN-16** The single-request test MUST display status, total duration, size, response headers, and a formatted body, truncated to 256 KB with a notice.
- **FR-SCN-17** Scenario saves MUST increment the revision.
- **FR-SCN-18** Runs MUST snapshot the full scenario definition. Editing a scenario later MUST NOT change historical runs.

### 6.4 Load profiles

- **FR-LP-01** The app MUST create, edit, duplicate, archive, and delete load profiles.
- **FR-LP-02** Presets MUST include smoke, load, stress, spike, and soak. Presets are starting points and are fully editable.
- **FR-LP-03** Supported executors: constant VUs, ramping VUs, and constant arrival rate.
- **FR-LP-04** A load profile MUST define warm-up seconds, executor configuration, and graceful stop seconds.
- **FR-LP-05** The editor MUST show a timeline of warm-up, stages, and ramp-down, plus total duration.
- **FR-LP-06** VU-based profiles MUST NOT display a fabricated request estimate. Arrival-rate profiles MUST display `rate × duration` and the estimated raw-artifact size.
- **FR-LP-07** Validation MUST enforce: at least one VU, duration and stages greater than zero, arrival rate greater than zero, pre-allocated VUs ≤ max VUs, and all values within environment caps.
- **FR-LP-08** The generated k6 scenario MUST set `gracefulRampDown` to 0 and `gracefulStop` to the profile value.

### 6.5 Objectives

- **FR-OBJ-01** Objectives MUST be editable as rows: metric, operator, value, unit, and abort-on-fail.
- **FR-OBJ-02** Supported metrics:
  - `latency_p50`, `latency_p90`, `latency_p95`, `latency_p99`, `latency_mean`, `latency_min`, `latency_max` (ms)
  - `throughput_rps` (requests/s)
  - `http_error_rate` (ratio, displayed as %)
  - `error_rate` (all failures, ratio)
  - `assertion_failure_rate` (ratio)
  - `timeout_rate` (ratio)
  - `connection_failure_rate` (ratio)
- **FR-OBJ-03** Latency and error objectives MUST use `<` or `≤`. Throughput MUST use `>` or `≥`. The UI restricts incompatible operators.
- **FR-OBJ-04** Objective values MUST be finite. Ratios MUST be between 0 and 1. Latency and throughput MUST be positive.
- **FR-OBJ-05** At least one latency objective and one error objective SHOULD be offered as defaults on a new experiment. Throughput MUST NOT be prefilled with a number.
- **FR-OBJ-06** Objectives MUST be evaluated over the measurement window, excluding warm-up.
- **FR-OBJ-07** Each objective MUST require at least 30 samples or attempts. Below that, its result is `insufficient_data`.
- **FR-OBJ-08** `abort_on_fail` MUST stop the run after the objective has been failing for three consecutive one-second buckets following the warm-up window.
- **FR-OBJ-09** Objectives MUST be run-level in the MVP. Per-step metrics are shown in the report but cannot be objectives.
- **FR-OBJ-10** The experiment editor MUST explain that p95/p99 are percentiles, not averages, and MUST display sample counts next to results.

### 6.6 Experiments

- **FR-EXP-01** The app MUST create, edit, duplicate, archive, and delete experiments.
- **FR-EXP-02** An experiment MUST reference a scenario, load profile, environment, objectives, and optional baseline run. All references MUST belong to the same project.
- **FR-EXP-03** The experiment editor MUST show a live summary of target host, total duration, VUs/rate, caps, and baseline.
- **FR-EXP-04** Saving MUST validate cross-entity references and block if any referenced entity is archived or missing.
- **FR-EXP-05** Starting a run MUST create a full snapshot before spawning the engine.
- **FR-EXP-06** Re-run MUST create a new run from the current configuration; it MUST NOT mutate the previous run.
- **FR-EXP-07** The baseline MUST default to the most recent passed run of the same experiment, if one exists.
- **FR-EXP-08** Setting a run as baseline MUST update the experiment and MUST be reflected in future reports.

### 6.7 Runs and live monitoring

- **FR-RUN-01** Only one run MAY be active per app instance. Starting another MUST be blocked with an actionable message.
- **FR-RUN-02** Run status transitions MUST follow the state machine in §9.5. Invalid transitions MUST be rejected and logged.
- **FR-RUN-03** A run MUST have an artifact directory containing snapshot, script, log, raw JSONL, summary, and engine metadata.
- **FR-RUN-04** Live metrics MUST appear within two seconds of the first sample and MUST never block the UI thread.
- **FR-RUN-05** Live charts MUST read one-second buckets. The UI MUST NOT render one element per raw sample.
- **FR-RUN-06** Stop MUST attempt a graceful k6 REST stop, wait for the graceful period plus five seconds, then force kill. The stop reason MUST be recorded.
- **FR-RUN-07** A watchdog MUST enforce maximum duration, maximum RPS for VU-based profiles, and minimum free disk space.
- **FR-RUN-08** Preflight MUST run before every start and MUST block on any failing check.
- **FR-RUN-09** Run history MUST support filtering by experiment, verdict, and date, with keyset pagination of 50 rows.
- **FR-RUN-10** Deleting a run MUST require confirmation, delete database rows, then delete artifacts. It MUST NOT delete a run that is active.
- **FR-RUN-11** The live view MUST show load-generator CPU and memory, labeled as the local generator, not the target.
- **FR-RUN-12** On app exit during a run, the app MUST require the user to stop the run first. Force-quit recovery MUST handle orphans on next launch.

### 6.8 Reports

- **FR-RPT-01** A report MUST show the verdict before any metric.
- **FR-RPT-02** The objectives table MUST show target, actual, pass/fail/inconclusive, sample count, and margin.
- **FR-RPT-03** Charts MUST include latency over time with objective line, throughput over time, error breakdown, and latency histogram.
- **FR-RPT-04** The report MUST show a per-step table with requests, p95, error rate, and sample count.
- **FR-RPT-05** The report MUST show deterministic findings with evidence references.
- **FR-RPT-06** The report MUST show the measurement window, warm-up duration, total attempts, and engine version.
- **FR-RPT-07** The report MUST show a configuration snapshot with secrets redacted.
- **FR-RPT-08** The report MUST offer "Copy k6 command" with secret placeholders, not secret values.
- **FR-RPT-09** The report MUST offer Compare and Re-run actions.
- **FR-RPT-10** An aborted run MUST NOT be reported as passed. It MUST show partial metrics and mark objectives not evaluated or insufficient.
- **FR-RPT-11** A run with fewer than 30 samples MUST show an inconclusive verdict when no objective failed.

### 6.9 Comparison

- **FR-CMP-01** The compare screen MUST allow selecting two to four runs from the same project.
- **FR-CMP-02** It MUST warn when runs differ in scenario hash, profile executor, environment class, or engine version.
- **FR-CMP-03** It MUST overlay latency and throughput on a shared elapsed-time axis.
- **FR-CMP-04** It MUST show deltas for p50, p95, p99, throughput, error rate, timeouts, and connection failures, with sample counts.
- **FR-CMP-05** Deltas MUST show absolute and percent change, except error-rate deltas which show percentage points.
- **FR-CMP-06** The screen MUST state that differences are not tested for statistical significance in the MVP.
- **FR-CMP-07** The user MUST be able to set a comparison run as the experiment baseline.

### 6.10 Settings and demo

- **FR-SET-01** Settings MUST detect k6 from a configured path, `BENCHLAB_K6_PATH`, `PATH`, and common install locations. It MUST show path, version, and support status.
- **FR-SET-02** Settings MUST show the data directory, open it in the OS file manager, and display disk usage.
- **FR-SET-03** Settings MUST offer dark theme (MVP only) and compact/comfortable density.
- **FR-SET-04** Settings MUST show whether persisted secrets are available. If the keychain is unavailable, secrets are session-only and the UI MUST say so.
- **FR-SET-05** Settings MUST include an About section with app version and third-party licenses.
- **FR-SET-06** The app MUST NOT send telemetry. This MUST be stated in Settings.
- **FR-DEMO-01** The demo project MUST be created idempotently and MUST NOT overwrite an existing demo.
- **FR-DEMO-02** The bundled mock target MUST run on localhost and serve deterministic endpoints.
- **FR-DEMO-03** The demo experiment MUST be runnable without editing and MUST produce a meaningful report.
- **FR-DEMO-04** The user MUST be able to reset or delete the demo project.

---

## 7. Information architecture and screens

### 7.1 Shell

```text
┌──────────────────────────────────────────────────────────────────────────┐
│ Top bar 48px: [project v] [breadcrumb]        [search / Ctrl K] [live] [New] │
├────────────┬─────────────────────────────────────────────────────────────┤
│ Rail 232px │ Content area                                                │
│            │                                                             │
│ Overview   │                                                             │
│ Experiments│                                                             │
│ Scenarios  │                                                             │
│ Profiles   │                                                             │
│ Runs       │                                                             │
│ Compare    │                                                             │
│            │                                                             │
│ [engine]   │                                                             │
│ [settings] │                                                             │
├────────────┴─────────────────────────────────────────────────────────────┤
│ Status bar 24px: engine · database · version · active run                │
└──────────────────────────────────────────────────────────────────────────┘
```

- Rail width 232 px, fixed. Content fills remaining width with 24 px gutters.
- Top bar includes a project selector, breadcrumb, command-palette trigger, active-run pill, and primary **New experiment** action.
- Status bar shows engine status (green dot + version or amber warning), data directory state, app version, and active run.
- Rail navigation: Overview, Experiments, Scenarios, Load profiles, Runs, Compare. Settings is pinned at the bottom.
- Schedules and Automation are absent in the MVP, not shown as broken placeholders.
- Native window title bar is retained. The app does not draw custom window controls in the MVP.
- Default window 1440×900 logical pixels. Minimum 1080×720. Size and position restored.

### 7.2 Screen specifications

| Screen | Purpose | Key elements | Required states |
|---|---|---|---|
| Welcome | First launch | Wordmark, one-line proposition, Create project, Open demo, recent projects, engine strip | No projects, engine missing, recent projects |
| Project overview | Orientation | Project header, environment selector, New experiment, p95 trend, recent runs table, experiments list | Empty, loading, populated, error |
| Experiments | Manage experiments | Search, table, row actions, New experiment | Empty, filtered empty, loading |
| Experiment editor | Configure and run | Target, load, objectives, safety, baseline, notes; sticky run summary | Valid, invalid, stale references, ready |
| Scenario editor | Build requests | Step list, step inspector, variables, test response | Empty, selected, validation errors, test running/success/failure |
| Load profile editor | Configure load | Preset cards, executor fields, timeline, derived duration | Valid, cap warning, cap error |
| Preflight | Confirm and start | Checklist, target summary, request estimate, authorization, production confirmation | Pass, warnings, blocked |
| Live run | Observe | Status strip, readouts, latency chart, objectives, error stream, log, stop | Starting, warming, running, stopping, finished, degraded |
| Run report | Understand | Verdict strip, meta, objectives table, charts, per-step, findings, config, artifacts | Passed, failed, inconclusive, aborted, error, partial |
| Compare | Compare runs | Run chips, compatibility banner, overlay charts, delta table, baseline action | 2 runs, 3–4 runs, incompatible, loading |
| Settings | Configure app | Engine, data, appearance, privacy, about | Detect success/failure, keychain unavailable |
| Command palette | Fast navigation | Search field, grouped results, shortcut hints | Empty query, results, no results |

### 7.3 Welcome

- Centered content column, max width 720 px.
- Wordmark "BenchLab" at 28 px, subtitle "API performance workbench" at 14 px secondary.
- One sentence: "Design repeatable API load tests, see whether they meet their objectives, and understand what changed."
- Primary button: **Create project**.
- Secondary button: **Try the demo**.
- Engine strip below: green "k6 2.2.0 detected" or amber "k6 not found" with **Set up** opening Settings.
- Recent projects list, three rows maximum, with last-opened time.
- No marketing illustrations, no feature grid, no stock screenshots.
- If a project exists, the app opens the last project overview instead of Welcome. Welcome remains reachable from the project switcher.

### 7.4 Project overview

- Header: project name (24 px), accent dot, environment selector, **New experiment** button.
- Primary content: a single p95 trend chart for the last 20 completed runs (line + points, objective line only if a consistent objective exists; otherwise no line) at 220 px height.
- Below: recent runs table with columns Verdict, Experiment, Started, Duration, p95, RPS, Errors. Row click opens report. Five rows plus **View all**.
- Right column, 320 px: "Experiments" list with last verdict and run count; "At a glance" with pass rate over the last 20 runs, total runs, and last run time. No more than four numbers.
- Empty state: "No runs yet. Create an experiment or try the demo." with two actions.

### 7.5 Experiments list

- Full-width table. Columns: Name, Scenario (revision), Load (summary), Environment, Last run, Verdict, p95, Updated.
- Search filters name and description. A verdict filter is available from the table header.
- Row hover reveals a menu: Run, Duplicate, Archive, Delete.
- Row click opens the experiment editor. A separate "last run" link opens the report.
- Empty state: "No experiments yet. An experiment combines a scenario, a load profile, and pass/fail criteria." Primary **New experiment**, secondary **Create scenario**.
- Archived experiments are hidden behind a "Show archived" toggle.

### 7.6 Experiment editor

Two columns on widths ≥ 1280 px; stacked below.

**Left column, sections**

1. **Target**
   - Scenario select with revision, **Edit** link, and step count.
   - Environment select with class pill and host.
   - Base URL preview, read-only.
2. **Load**
   - Profile select with preset name and summary ("50 VUs · ramp 60 s · hold 4 min").
   - **Edit profile** opens the profile editor sheet.
   - Timeline strip: warm-up, ramp, hold, ramp-down, total duration.
3. **Objectives**
   - Table: Metric, Operator, Value, Unit, Abort on fail, Remove.
   - **Add objective** row with a metric select. Throughput is not preselected.
   - Helper text: "Objectives are evaluated after warm-up. Percentiles are not averages."
4. **Baseline**
   - Searchable select of previous runs of this experiment; shows verdict, p95, and date.
   - "Use latest passed run" action. "None" allowed.
5. **Notes**
   - Free text, 2,000 characters, included in reports.

**Right column, sticky summary**

- Experiment name (editable inline), save state.
- Target host and environment class.
- Total duration, VUs or arrival rate, warm-up.
- Caps: max VUs, max RPS, max duration. Any requested value above a cap is shown as a blocking row.
- Engine status and version.
- Objective count and a compact list.
- Primary **Run** button, disabled with a reason when invalid.
- Secondary **Save**.
- Last saved timestamp.

**Preflight sheet**

- Opens on Run. Checklist rows with icons:
  - Engine found and supported.
  - Scenario valid.
  - Environment valid; secrets resolved.
  - Target reachable; status code shown.
  - Within safety caps.
  - Disk space sufficient.
  - Baseline selected (info only).
- Summary sentence: "This will send approximately N requests over M minutes to `host`."
- Authorization checkbox: "I have authorization to test this target." Required for staging and production; hidden for local.
- Production: input field requiring the environment name.
- Buttons: Cancel, **Start run**. Start disabled until all blocking checks pass and confirmation requirements are met.
- Warnings (for example, 401 from preflight, missing baseline, low disk estimate) do not block but are shown above Start.

### 7.7 Scenario editor

**Header:** scenario name, revision, validation badge, **Save**, **Test step**.

**Step list (left, 320 px)**

- Cards in execution order. Each shows method chip, step name, path, assertion count, extraction count, and a validation badge.
- Selected card has an accent left border and raised background.
- **Add step** at the bottom.
- Reorder via up/down buttons in the card menu; keyboard accessible. Drag-and-drop is optional.
- Duplicate, delete, and insert-after actions in the card menu.
- A "Variables" entry above the list opens the variables panel.

**Inspector (right)**

- Step name.
- Method + URL row.
- Timeout (ms) and think time (none/fixed/range).
- Tabs: Headers, Query, Body, Assertions, Extract.
- Headers/Query: key/value rows, add/remove, monospace values, secret placeholder hint.
- Body: type select (none/JSON/text), multiline monospace editor, **Format JSON** action, character count.
- Assertions: list of rows. Each row has kind, fields, and remove. Add assertion menu.
- Extract: rows with variable name, source, expression.
- **Send request** below the step form. Response panel: status pill, duration, size, tabs Headers/Body, copy body, truncation notice.
- Secrets are masked as `••••••` in request preview. The response may contain sensitive data; it is shown but never persisted.

**Variables panel**

- Rows: key, value (masked for secret), source (environment or scenario), delete. Environment variables are read-only and shown for reference.
- Scenario variables are editable. Secret scenario variables are not supported in the MVP; secrets live on environments.
- Validation messages for duplicate/reserved names.

**Empty state:** "Scenarios are ordered HTTP steps. Add the first request to begin." with **Add step**.
**Validation:** errors appear on the card, in the inspector, and in the header badge. Save is allowed for drafts but Run is blocked until valid. A "draft" is simply an invalid saved scenario; it remains listable.

### 7.8 Load profile editor

- Presented as a right-side sheet or a full screen on narrow windows.
- Preset cards at the top: Smoke, Load, Stress, Spike, Soak, Custom. Selecting a preset fills fields and marks the profile "Based on X".
- Fields depend on executor:
  - Constant VUs: VUs, duration.
  - Ramping VUs: stage rows (duration, target VUs), add/remove, total duration.
  - Constant arrival rate: rate, time unit (1 s), duration, pre-allocated VUs, max VUs.
- Warm-up seconds and graceful stop seconds.
- Timeline strip showing phases and warm-up overlay.
- Derived values: total duration, maximum VUs/rate, estimated requests (arrival only), estimated raw artifact size.
- Cap warnings in amber, cap violations in red with the exact environment cap.
- Save/Cancel. A profile used by an experiment shows "Used by N experiments" and requires confirmation before destructive edits? MVP: edits are allowed; historical runs are unaffected, and experiments show the new revision.

### 7.9 Live run

**Top strip**

- Run short ID, experiment name, environment class pill.
- Status: Starting, Warming up, Running, Stopping, Finalizing, Finished.
- Elapsed / total duration in monospace.
- **Stop** button (danger outline). Overflow menu: Force stop.
- If degraded: amber "Metrics degraded" badge with tooltip.

**Readouts (four columns)**

- Requests/s with a 60-second sparkline.
- p95 latency with target line and sparkline.
- Error rate with sparkline.
- Active VUs with sparkline.
- Each readout uses tabular numerals, 28 px value, 11 px label, and a 56×20 sparkline.

**Main chart**

- Latency over time, p50/p95/p99, 260 px height, full width.
- Objective lines dashed in the objective color, labeled at the right edge.
- Warm-up region shaded and labeled.
- Legend toggles series. Hover crosshair with a shared tooltip listing elapsed time and all visible series.
- Chart y-axis auto-scales to the running maximum with 10% headroom and only grows during a run to avoid jumpiness.

**Right column (320 px)**

- **Objectives**: live status per objective, actual value, target, sample count. Status: pending, passing, failing, insufficient.
- **Errors**: last ten error classes with count and last status code. Aggregated, not one row per request.
- **Engine**: last five log lines, expandable to a log panel. No secrets.
- **Generator**: CPU %, RSS MB.

**Bottom**

- Progress bar with warm-up and measurement segments.
- Total requests, failed, timeouts, bytes received.
- If the user stops: inline confirmation sheet with **Stop gracefully** and **Force stop** (danger, explains possible data loss).

**Finished transition**

- On completion, a toast shows verdict. The view transitions to the report after 800 ms, or immediately if the user clicks **View report**. A "Stay on live" control is not needed in MVP.

### 7.10 Run report

**Verdict strip**

- Full-width 64 px strip, tinted by verdict, not a giant card.
- Icon + verdict label + one sentence:
  - "Passed 4 of 4 objectives."
  - "Failed 1 of 4 objectives: p99 exceeded 500 ms."
  - "Aborted by you after 2:14. Objectives were not evaluated."
  - "Inconclusive: p99 had 28 samples; 30 required."
  - "Engine error: k6 exited with code 2."
- Secondary actions right-aligned: Compare, Re-run, overflow (Delete, Copy CLI, Open artifact folder).

**Meta row**

- Started (local time), duration, measurement window, engine + version, environment, scenario revision, profile revision, reproducibility hash (short).
- Warm-up excluded is stated explicitly.

**Objectives table**

Columns: Objective, Target, Actual, Result, Samples, Margin. Failed rows tinted; result uses icon + text, not color alone. Margin example: "34 ms under target" or "82 ms over".

**Charts**

- Row 1: latency over time, full width, 280 px, p50/p95/p99 + objective line + warm-up band.
- Row 2: throughput over time (60%), error rate over time or error breakdown (40%).
- Row 3: latency histogram (50%) and per-step p95 bars (50%).
- Every chart has a title, unit, and sample count. Empty data shows "No samples in this window."

**Per-step table**

Columns: Step, Requests, p50, p95, p99, Error rate, Timeouts, Samples. Sortable by p95 and error rate. Clicking a row filters the latency chart to that step (MVP: highlights the step; chart filtering is Phase 2).

**Findings panel**

- Deterministic cards, max 6, ordered by severity.
- Each card: severity dot + title, one-paragraph explanation, evidence chips.
- Evidence chip click scrolls to the related objective/chart/step.
- Empty state: "No notable findings. The run met its objectives and no anomalies were detected." Do not invent content.

**Configuration**

- Collapsible sections: Environment, Scenario, Load profile, Objectives, Safety, Engine.
- Secrets shown as `••••••` with fingerprint suffix.
- "Copy k6 command" and "Open artifacts".

**Artifacts**

- Snapshot JSON, script.js, raw.jsonl, summary.json, k6.log, engine.json with sizes.
- Open folder action. Raw file is authoritative.

### 7.11 Compare

- Header: experiment name, selected runs, **Set baseline**.
- Run selector: chips with run short ID, verdict, p95, date. Up to four. Adding a fifth replaces the oldest selection after confirmation.
- Compatibility banner if any pair differs in scenario hash, profile executor, environment class, or engine version. Text: "These runs used different configurations. Differences may not be attributable to the target."
- Charts: latency overlay (p95 per run; expanded series toggles), throughput overlay. Shared elapsed axis. Runs use project accent palette + distinct dash patterns.
- Delta table: rows for p50, p95, p99, throughput, HTTP error rate, timeout rate, connection failure rate; columns per run with absolute and percent delta versus the first selected run.
- Baseline summary: "v2 reduced p95 by 18% (312 → 256 ms), but p99 increased by 12% (480 → 538 ms)." Generated from the delta table, deterministic.
- Note: "No significance test in this version. Compare sample counts before treating small differences as real."
- Empty state: "Select at least two completed runs to compare."

### 7.12 Settings

Sections in a single scroll:

1. **Engine**
   - Status, path, version, supported range.
   - **Choose k6 binary** (file picker), **Re-detect**, **Copy install command**.
   - Windows command: `winget install --id GrafanaLabs.k6 -e`.
   - Link to k6 installation docs.
2. **Data**
   - Data directory path, **Open folder**, database size, artifacts size, total disk free.
   - Retention: keep all runs (MVP default), with a warning at 5 GB. Automatic pruning is Phase 2.
3. **Appearance**
   - Theme: Dark (only option, shown as selected).
   - Density: Comfortable (default) / Compact.
   - UI scale follows the OS in the MVP.
4. **Privacy**
   - "BenchLab does not collect telemetry. No data leaves this machine unless you connect an AI provider in a future version."
   - Secret storage status: OS keychain or session-only.
5. **About**
   - Version, build date, licenses, link to third-party notices.
   - Engine acknowledgements: k6 is an external AGPL-3.0 program.

### 7.13 Command palette

- Overlay centered at 560 px width, top 18% of window.
- Search input auto-focused. Results grouped: Actions, Projects, Experiments, Runs, Navigation.
- Fuzzy matching on labels and keywords. Up/down to move, Enter to run, Esc to close.
- Actions include: New project, New experiment, New scenario, New load profile, Run last experiment, Open settings, Open data folder, Re-detect k6, Toggle density.
- Recent items first when the query is empty.
- No results: "No matches. Try an action like 'new experiment'."
- Keyboard shortcut hint shown at the bottom: `↑↓ navigate · ↵ open · esc close`.

### 7.14 Empty, loading, and error states

| Context | Loading | Empty | Error |
|---|---|---|---|
| Project list | Three skeleton rows, no shimmer | "No projects yet." + Create | "Could not read the workspace." Retry + Open data folder |
| Run history | Table skeleton | "No runs for this experiment." + Run | "Run history unavailable." Retry |
| Charts | Axis frame + "Waiting for first sample…" | "No samples in this window." | "Chart data could not be read." Details |
| Scenario test | Button spinner, response panel skeleton | "Send the request to see a response." | Status, error class, retry |
| Engine | "Checking k6…" | "k6 not found." + Set up | "k6 could not be executed." Path + log |
| Preflight | Checklist skeleton | n/a | Blocking item with fix action |
| Report | Route skeleton | "This run produced no samples." | Verdict error + artifact folder |

Rules:

- No infinite spinners. Every async action has a timeout and a retry or cancel.
- Errors state what failed, why, and the next action. No raw backtraces in the UI.
- Toasts: maximum three, 4 seconds, one action. Errors that require action use inline banners instead.

---

## 8. Visual system

### 8.1 Direction

**Instrument panel, not dashboard.** The interface should feel like a precision measurement tool: dark graphite surfaces, hairline separators, restrained accent color, tabular numerals, charts treated as first-class content. Dense but calm. No gradients, glassmorphism, oversized rounded cards, emoji, or decorative illustrations.

- The primary content of a report is a chart and a verdict, not a grid of KPI cards.
- Use full-width sections with 1 px separators instead of floating cards.
- Project accent identifies context; it is never used to encode data.
- Data colors are semantic and consistent across all charts.
- Motion is functional: state transitions and live indicators only.

### 8.2 Tokens

| Token | Value | Use |
|---|---|---|
| `bg.app` | `#0B0D10` | Window background |
| `bg.surface` | `#111418` | Main panels |
| `bg.raised` | `#171B21` | Inputs, hovered rows, overlays |
| `bg.sunken` | `#080A0C` | Code/JSON editors, chart plot areas |
| `border.subtle` | `#1F242B` | Section dividers |
| `border.default` | `#2A313A` | Inputs, table borders |
| `border.strong` | `#3A4350` | Focus-adjacent, selected |
| `text.primary` | `#E8EAED` | Primary text |
| `text.secondary` | `#A2AAB6` | Labels, secondary |
| `text.muted` | `#6C7684` | Captions, disabled |
| `accent.blue` | `#5B8CFF` | Primary action, focus |
| `accent.teal` | `#2FB6A8` | Project accent |
| `accent.violet` | `#A78BFA` | Project accent |
| `accent.amber` | `#E6A23C` | Project accent, warning |
| `accent.rose` | `#E06C75` | Project accent |
| `success` | `#3FB950` | Passed |
| `warning` | `#D29922` | Warnings |
| `danger` | `#F05252` | Failed, destructive |
| `info` | `#58A6FF` | Informational |
| `chart.p50` | `#7C8CF8` | p50 |
| `chart.p90` | `#56B4E9` | p90 |
| `chart.p95` | `#E69F00` | p95 |
| `chart.p99` | `#D55E00` | p99 |
| `chart.error` | `#D55E00` | Errors |
| `chart.success` | `#009E73` | Successful traffic |
| `chart.grid` | `rgba(255,255,255,0.07)` | Grid lines |

Latency series use the Okabe–Ito colorblind-safe palette. Error and p99 may share a hue only when they never appear in the same chart; prefer distinct dash patterns when they do.

### 8.3 Typography

- UI: **Inter** variable, bundled. Fallback: system UI.
- Numerals and code: **JetBrains Mono**, bundled. All metrics, timings, status codes, IDs, and JSON use mono.
- Scale: 11 caption, 12 label, 13 body, 15 section title, 20 page title, 28 metric, 32 report metric.
- Line height: 1.4 body, 1.2 headings, 1.0 numeric readouts.
- Weight: 400 body, 500 labels, 600 headings and metric values. Avoid 700 except the verdict label.
- Numerals use tabular figures. Units are 11 px, muted, separated by a thin space.
- No letter-spacing on body text. Uppercase only for 11 px overline labels.

### 8.4 Layout and density

- 4 px spacing scale. Section padding 24 px. Card internal padding 16 px. Table row 36 px comfortable, 32 px compact.
- Rail 232 px; right inspector 320 px; content min width 520 px.
- Maximum report width 1440 px, centered; charts may span full content width.
- Hairline borders (1 px) do all separation. Shadows only for overlays and the command palette.
- Radius 6 px for controls, 8 px for overlays, 4 px for chips. Never 16 px cards.
- Focus ring: 2 px accent, 1 px offset, visible on keyboard focus only.
- Scrollbars are thin, low-contrast; content remains usable without hover.

### 8.5 Components

Build custom components in `benchlab-ui`. Do not adopt a UI kit's visual language.

| Component | Notes |
|---|---|
| Button | Primary, secondary, ghost, danger; sm 28 px, md 32 px; icon + label; loading state preserves width |
| IconButton | 28/32 px, tooltip with label and shortcut |
| TextInput | Single-line and multiline; selection, clipboard, IME; password masking; validation message; mono variant |
| Select | Searchable dropdown, keyboard navigation, checkmark on selected |
| Checkbox / Switch | Large hit area; label click toggles |
| Table | Header, sort indicator, row hover, selected row, sticky header, virtualized with `uniform_list` |
| KeyValueEditor | Add/remove rows, drag optional, paste multi-line `key: value` support |
| Tabs | Underline style, 1 px separator, keyboard arrows |
| Modal / Sheet | Focus trap, Esc closes, backdrop click closes only non-destructive dialogs |
| ConfirmDialog | Title, consequence sentence, destructive action requires explicit label |
| Banner | Info, warning, danger; one action maximum |
| Toast | Bottom-right, max three, 4 s, one action |
| Badge / Pill | Verdict, environment class, method; icon + text |
| Tooltip | 200 ms delay, 8 px offset, no interactive content |
| ProgressBar | Determinate only; warm-up segment distinguished |
| EmptyState | Short title, one sentence, one primary and at most one secondary action |
| CodeBlock | Mono, sunken background, copy action, line wrapping off |

Text input is a critical path. Build one reusable `TextInput` entity implementing GPUI's `EntityInputHandler`, including selection, clipboard, IME, password masking, and multiline. Time-box it to one day. If it blocks progress, adopt a minimal third-party input primitive behind the same internal API, but keep the theme and behavior owned by BenchLab.

Do not show raw implementation details in the UI. Shortcuts use `Ctrl` on Windows and Linux; use the platform modifier label where relevant.

### 8.6 Chart language

- Canvas-based, custom-drawn. No chart crate.
- Plot area 1 px grid at `chart.grid`; axes hairline; no chart borders.
- Series: 1.5 px lines, 2 px when hovered. No area fill for latency lines. Area fill allowed for error volume, low opacity.
- Thresholds: 1 px dashed line in the semantic color, label at the right edge, never across the legend.
- Warm-up: sunken band with 8% white overlay and a rotated label only if width permits; otherwise a legend entry "warm-up excluded".
- Hover: vertical crosshair, 4 px dots, shared tooltip. Tooltip shows elapsed time, series values with units, and request count for the bucket.
- X-axis: elapsed `mm:ss`, tick spacing chosen from 15 s, 30 s, 1 m, 5 m, 15 m, 1 h. No rotated labels.
- Y-axis: "nice" ticks (1/2/5 × 10^n), units in the axis title. Latency adaptive ms/s. Throughput `req/s`, `k req/s` above 10,000.
- Histogram: log-spaced bins (50), bars 2 px gap, p50/p95/p99 vertical markers with labels. No pie charts.
- Error breakdown: horizontal bars by class and top status codes. Not a donut.
- Decimation: at most two points per horizontal pixel; preserve min and max per pixel column, plus the last sample. Never average away spikes. At 1 s buckets, 5 minutes and 1 hour both render acceptably.
- Empty: axes and grid remain, with the empty message centered in the plot.
- Live y-axis grows only; reset on metric change or new run.
- Comparison: up to four runs, distinct colors + dash patterns, legend labels include run short ID and verdict. First selected run is the reference; deltas are relative to it.
- Accessibility: every chart has a text summary above or below (for example, "p95 184 ms, target 250 ms, 84,213 samples"). Color is never the only encoding.

Chart implementation:

```rust
pub struct ChartFrame { pub plot: Bounds<Pixels>, pub x: Scale, pub y: Scale }
pub struct Series { pub id: &'static str, pub label: String, pub color: Rgba, pub width: Pixels, pub dash: Option<Vec<Pixels>>, pub points: Vec<Option<f64>> }
```

- Compute plot bounds from the container; reserve 56 px left, 16 px right, 24 px top, 28 px bottom.
- Draw grid, bands, threshold lines, and series with `canvas` + `PathBuilder`; draw axis labels and tooltips as absolutely positioned GPUI text for crisp rendering.
- Hover state lives in the chart view entity; mouse move updates nearest bucket and calls `cx.notify()`.
- Keep chart data immutable per frame; do not clone full histories on every render.

### 8.7 Motion

- 120 ms ease-out for hover, selection, and overlay entry. 180 ms for sheets.
- Live status dot pulses at 2 s only while running; no pulsing elsewhere.
- No skeleton shimmer. Use static blocks at `bg.raised`.
- Reduced motion setting is Phase 2; respect OS animation settings where GPUI exposes them.

### 8.8 Accessibility

- Contrast at least WCAG AA for text and controls.
- All actions reachable by keyboard. Visible focus ring. Tab order follows visual order.
- Tables expose row actions via keyboard, not hover only.
- Charts have text summaries and legends; tooltips are not the only source of values.
- Do not use color alone for verdicts or errors: icon + label.
- Minimum hit target 28×28 px; primary controls 32 px.
- Support Windows display scaling 100%, 125%, 150%, 200%. Test at minimum window size.
- Error messages are programmatically associated with inputs (GPUI: visible inline text adjacent to field; focus moves to first error on save).

---

## 9. Architecture

### 9.1 Platform statement

GPUI is multiplatform: it has macOS, Linux (Wayland/X11), and Windows backends. Windows support is newer than macOS/Linux but present in GPUI 0.2.x.

- **Primary acceptance target:** Windows 10/11 x64, MSVC toolchain. This machine already has Rust 1.96.0 and Visual Studio 2022 Build Tools.
- **Secondary supported target:** Linux X11/Wayland, including WSLg for development. CI builds and tests it.
- **Best-effort:** macOS. Code should compile, but it is not acceptance-tested in this project.
- Platform-specific code is isolated to: app data paths, keychain access, process termination fallbacks, and file-manager opening. Everything else is portable.

### 9.2 Crate layout

```text
BenchLab/
├── Cargo.toml                  # workspace
├── Cargo.lock                  # committed
├── rust-toolchain.toml         # 1.96.0, rustfmt, clippy
├── crates/
│   ├── benchlab-core/          # domain types, validation, IDs, metric math
│   ├── benchlab-store/         # SQLite, migrations, artifacts, secrets
│   ├── benchlab-engine/        # engine trait, k6 adapter, supervisor, parser
│   ├── benchlab-analysis/      # objectives, summaries, comparison, findings
│   ├── benchlab-ui/            # theme, components, charts, views
│   └── benchlab-app/           # binary, wiring, services, demo target
├── assets/
│   ├── fonts/                  # Inter, JetBrains Mono
│   ├── icons/                  # Lucide SVGs
│   └── demo/                   # demo project seed
├── migrations/
│   └── 0001_init.sql
├── tests/
│   ├── fixtures/k6/            # recorded raw.jsonl + summary.json
│   └── integration/
└── SPEC.md
```

| Crate | Responsibility | May depend on |
|---|---|---|
| `benchlab-core` | Entities, enums, validation, IDs, metric types, formulas | serde, uuid, chrono, thiserror |
| `benchlab-store` | DB pool, migrations, repositories, artifact paths, keychain | core, rusqlite, tokio-rusqlite, keyring, secrecy |
| `benchlab-engine` | `LoadEngine`, k6 discovery, script generation, process, JSONL parser, aggregator | core, tokio, serde_json, reqwest, sysinfo |
| `benchlab-analysis` | Final statistics, objective evaluation, comparison, findings | core |
| `benchlab-ui` | GPUI views, components, charts, theme, assets | core, gpui |
| `benchlab-app` | Startup, services, run orchestration, demo target, CLI | all |

`benchlab-ui` MUST NOT depend on `benchlab-store` or `benchlab-engine` directly. It consumes view models and service traits exposed by `benchlab-app`. This keeps GPUI churn isolated.

### 9.3 Process and threading model

```text
GPUI UI thread
  AppState entity
    ├── Route + project
    ├── views (entities)
    ├── watch::Receiver<LiveSnapshot>  (UI reads latest, 10 Hz max)
    └── actions -> services

Services (Arc, background)
  ├── Store            tokio-rusqlite dedicated thread
  ├── RunService       orchestrates preflight, engine, finalization
  ├── K6Supervisor     tokio::process child, REST stop, watchdog
  ├── ArtifactWriter   disk I/O
  ├── Tailer thread    reads raw.jsonl -> parser -> aggregator
  └── DbWriter task    batches buckets every 5 s
```

Rules:

1. **The UI thread MUST NOT** read files, query SQLite, spawn processes, parse JSONL, or compute heavy statistics.
2. **The load generator MUST run in a separate OS process.** k6 is a child process. A future native engine may run in a worker process; it must not run on the UI thread.
3. **The k6 child process environment** carries secrets. The command line never does.
4. **Live updates are coalesced.** The aggregator publishes a `watch<LiveSnapshot>`; the UI foreground task checks at 100 ms and repaints at most 10 times per second. State transitions and errors use a separate unbounded event channel for ordering.
5. **DB writes are batched.** One-second buckets are buffered and flushed every 5 s or 20 buckets, whichever comes first. The final transaction is atomic.
6. **Cancellation is explicit.** A `CancellationToken` per run stops the watchdog, tailer, and DB writer.
7. **No `Mutex` held across `.await`.** Use message passing or scoped locks.
8. **Raw JSONL is authoritative.** DB aggregates are derived and can be rebuilt from artifacts.

### 9.4 GPUI state management

- `AppState` is a GPUI entity holding route, current project, theme, toasts, and engine status.
- Each screen is its own entity (`ProjectsView`, `ExperimentsView`, `ScenarioEditor`, `RunLiveView`, `RunReportView`, `CompareView`, `SettingsView`).
- `AppServices` is set as a GPUI global: `Arc<Store>`, `Arc<RunService>`, `Arc<Analysis>`, `Arc<EngineRegistry>`, `Arc<AppPaths>`.
- Services return `Result<T, AppError>`; views map errors to inline messages or toasts.
- Subscriptions are stored on the view (`Vec<Subscription>`) and dropped with the view.
- Use `cx.background_executor().spawn` for blocking/cpu work and `cx.spawn` for foreground state updates. Use `cx.entity().update` and `cx.notify` from the foreground task.
- Route enum:

```rust
enum Route {
  Welcome,
  ProjectOverview { project_id: Uuid },
  Experiments { project_id: Uuid },
  ExperimentEditor { project_id: Uuid, experiment_id: Option<Uuid> },
  ScenarioEditor { project_id: Uuid, scenario_id: Uuid },
  Profiles { project_id: Uuid },
  RunLive { run_id: Uuid },
  RunReport { run_id: Uuid },
  Compare { project_id: Uuid, runs: Vec<Uuid> },
  Settings,
}
```

- Navigation replaces the content view; the shell persists. Back/forward history is not required in the MVP; breadcrumbs provide navigation.
- Inline editing saves on blur or explicit Save. The experiment name saves on blur; scenario/profile forms require explicit Save.
- Unsaved-changes guard: if a form is dirty, navigation or window close opens a confirm dialog with Save / Discard / Cancel.

### 9.5 Run lifecycle

```text
              ┌──────────────┐
              │  preflight   │
              └──────┬───────┘
                     │ pass
              ┌──────▼───────┐
              │   starting   │  write snapshot, script, spawn k6
              └──────┬───────┘
                     │ first sample / REST ready
              ┌──────▼───────┐
              │   running    │◄──────────────┐
              └───┬──────┬───┘               │
        stop/cap  │      │ engine exit       │
              ┌───▼──────▼───┐               │
              │  stopping    │               │
              └──────┬───────┘               │
                     │ exit                  │
              ┌──────▼───────┐               │
              │  finalizing  │───────────────┘
              └──────┬───────┘
                     │
        ┌────────────┼────────────┬─────────────┐
        ▼            ▼            ▼             ▼
    completed      failed       aborted      interrupted
```

Statuses:

- `preflight` - checks and confirmation.
- `starting` - snapshot and artifacts written, process spawning, waiting for REST/metrics.
- `running` - samples arriving.
- `stopping` - stop requested; waiting for k6 to exit.
- `finalizing` - exact statistics, objectives, findings, DB finalization.
- `completed` - finished with a verdict of passed, failed, or inconclusive.
- `failed` - engine error, no valid result.
- `aborted` - user stop, cap violation, max duration, low disk, target authorization revoked.
- `interrupted` - app crash or forced quit; recovered on next start.

Verdicts: `passed`, `failed`, `inconclusive`, `aborted`, `error`.

Rules:

- `aborted` runs never receive `passed`, even if partial metrics meet objectives.
- `completed` with zero objectives is allowed and labeled "Completed, no objectives".
- `failed` means an objective failed or the engine failed. Use `error` verdict for preflight/engine failures; use `failed` for objective failure.
- A run with an objective result `insufficient_data` and no failed objective gets `inconclusive`.

### 9.6 Error handling

- `AppError` variants carry a user-facing message, a remediation hint, and a technical detail for logs.
- UI never displays a backtrace. Fatal errors show a blocking panel with **Retry**, **Open logs**, and **Copy details**.
- Transient form errors are inline. Toasts are for completed background actions.
- Engine stderr is retained and the last 100 lines are shown in the report's artifacts section.
- Every async action has a timeout: preflight 10 s per check, engine start 15 s, stop wait graceful + 5 s, DB query 5 s.
- Panics are caught at the top of background tasks; write a crash log with the run ID and show a non-fatal banner. A panic in the UI thread writes a crash log and exits after showing a native error dialog.

### 9.7 Crash recovery and single instance

- Acquire an exclusive lock file at `<data_dir>/benchlab.lock` on startup. If locked, show "BenchLab is already running" and exit. Do not attempt to focus the other window in the MVP.
- On startup, inspect runs in `preflight`, `starting`, `running`, `stopping`, or `finalizing`.
- For each, read `engine.json` {pid, port, started_at}. If the PID is alive and its executable is k6 and its command line contains the run marker, attempt a REST stop; wait 5 s; kill if needed.
- Recompute aggregates from raw JSONL if present. Mark the run `aborted` with reason `app_restart` when samples exist, otherwise `failed` with reason `interrupted_before_samples`.
- Show one toast: "Recovered 1 interrupted run."
- Never auto-resume or auto-run.

### 9.8 Dependency choices

Pin exact versions in `Cargo.lock`. Indicative versions:

| Purpose | Crate | Notes |
|---|---|---|
| UI | `gpui = "=0.2.2"` | Pin exact; validate on Windows first |
| Async | `tokio` | `rt-multi-thread`, `process`, `sync`, `time`, `fs`, `io-util`, `macros` |
| Errors | `anyhow`, `thiserror` | anyhow in app/engine edges; thiserror in core |
| Serialization | `serde`, `serde_json` | JSON columns, script config, artifacts |
| IDs | `uuid` | v7, serde |
| Time | `chrono` | UTC storage, local display |
| DB | `rusqlite`, `tokio-rusqlite` | bundled SQLite; dedicated DB thread |
| HTTP | `reqwest` | rustls, json, stream; preflight and Phase 2 AI |
| Percentiles (live) | `hdrhistogram` | 3 significant figures |
| Process info | `sysinfo` | k6 PID/orphan checks, generator CPU/RSS |
| Secrets | `keyring`, `secrecy`, `zeroize` | OS keychain; no plaintext |
| Hashing | `sha2`, `hex` | Config hash, secret fingerprint |
| Logging | `tracing`, `tracing-subscriber`, `tracing-appender` | File + console |
| File dialogs | `rfd` | k6 path, OpenAPI (Phase 2) |
| Open folder | `open` | Data/artifact folder |
| Regex | `regex` | Validation; runtime uses JS RegExp |
| URL | `url` | Parse/validate hosts |
| Base64 | `base64` | Basic auth |
| Demo/test server | `axum` | Local mock target; dev/demo only |
| Dev | `insta`, `tempfile`, `serial_test` | Snapshots, temp dirs, serialized k6 tests |

Do not add a chart crate, a component kit, or a full ORM. Do not add SQLx.

---

## 10. Domain model

### 10.1 Core types

```rust
pub type Id = uuid::Uuid;

pub struct Project {
  pub id: Id,
  pub name: String,
  pub description: String,
  pub accent: Accent,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub archived_at: Option<DateTime<Utc>>,
}

pub enum EnvironmentKind { Local, Staging, Production }

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
}

pub struct Variable {
  pub key: String,
  pub value: Option<String>,       // None for secrets
  pub secret: bool,
  pub secret_ref: Option<Id>,      // metadata row; value in keychain
}

pub enum AuthConfig {
  None,
  Bearer { token_var: String },
  Basic { username_var: String, password_var: String },
  Header { name: String, value_var: String },
}

pub struct SafetyCaps {
  pub max_vus: u32,
  pub max_rps: Option<u32>,
  pub max_duration_s: u32,
  pub require_confirmation: bool,
}

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

pub struct Step {
  pub id: Id,
  pub name: String,
  pub method: Method,
  pub url: String,                 // relative or absolute; {{vars}} allowed
  pub headers: Vec<KeyValue>,
  pub query: Vec<KeyValue>,
  pub body: Body,
  pub timeout_ms: u64,
  pub think_time: ThinkTime,
  pub expected_status: Vec<u16>,
  pub assertions: Vec<Assertion>,
  pub extract: Vec<Extraction>,
}

pub enum Body {
  None,
  Json { template: String },
  Text { template: String },
}

pub enum ThinkTime {
  None,
  Fixed { ms: u64 },
  Range { min_ms: u64, max_ms: u64 },
}

pub enum Assertion {
  StatusIn { values: Vec<u16> },                    // also stored in expected_status
  HeaderEquals { name: String, value: String },
  BodyContains { value: String },
  BodyRegex { pattern: String },
  JsonPathEquals { path: String, value: serde_json::Value }, // scalar only
  MaxDurationMs { value: u64 },
  MaxSizeBytes { value: u64 },
}

pub enum Extraction {
  JsonPath { variable: String, path: String },
  Regex { variable: String, pattern: String, group: u8 },
  Header { variable: String, name: String },
  Status { variable: String },
}

pub struct LoadProfile {
  pub id: Id,
  pub project_id: Id,
  pub name: String,
  pub revision: u32,
  pub kind: ProfileKind,           // Smoke, Load, Stress, Spike, Soak, Custom
  pub executor: Executor,
  pub warmup_s: u64,
  pub graceful_stop_s: u64,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub archived_at: Option<DateTime<Utc>>,
}

pub enum Executor {
  ConstantVus { vus: u32, duration_s: u64 },
  RampingVus { stages: Vec<Stage> },
  ConstantArrivalRate { rate: u32, time_unit_s: u64, duration_s: u64, pre_allocated_vus: u32, max_vus: u32 },
}

pub struct Stage { pub duration_s: u64, pub target_vus: u32 }

pub struct Objective {
  pub id: Id,
  pub metric: MetricKind,
  pub operator: Operator,          // Lt, Lte, Gt, Gte
  pub value: f64,
  pub abort_on_fail: bool,
}

pub enum MetricKind {
  LatencyP50, LatencyP90, LatencyP95, LatencyP99, LatencyMean,
  LatencyMin, LatencyMax, ThroughputRps, HttpErrorRate, ErrorRate,
  AssertionFailureRate, TimeoutRate, ConnectionFailureRate,
}

pub struct Experiment {
  pub id: Id,
  pub project_id: Id,
  pub name: String,
  pub description: String,
  pub scenario_id: Id,
  pub profile_id: Id,
  pub environment_id: Id,
  pub objectives: Vec<Objective>,
  pub baseline_run_id: Option<Id>,
  pub notes: String,
  pub created_at: DateTime<Utc>,
  pub updated_at: DateTime<Utc>,
  pub archived_at: Option<DateTime<Utc>>,
}
```

### 10.2 Run types

```rust
pub enum RunStatus {
  Preflight, Starting, Running, Stopping, Finalizing,
  Completed, Failed, Aborted, Interrupted,
}

pub enum Verdict { Passed, Failed, Inconclusive, Aborted, Error }

pub struct Run {
  pub id: Id,
  pub experiment_id: Id,
  pub project_id: Id,
  pub status: RunStatus,
  pub verdict: Option<Verdict>,
  pub trigger: RunTrigger,          // Manual, Rerun, Demo, Recovery
  pub started_at: DateTime<Utc>,
  pub ended_at: Option<DateTime<Utc>>,
  pub engine: EngineInfo,           // id + version
  pub snapshot: RunSnapshot,
  pub summary: Option<RunSummary>,
  pub objectives: Vec<ObjectiveResult>,
  pub error: Option<String>,
  pub artifact_dir: PathBuf,        // relative to data dir
  pub host: HostInfo,
}

pub struct RunSnapshot {
  pub schema_version: u32,
  pub benchlab_version: String,
  pub created_at: DateTime<Utc>,
  pub project: ProjectRef,
  pub environment: EnvironmentSnapshot,
  pub scenario: Scenario,
  pub profile: LoadProfile,
  pub objectives: Vec<Objective>,
  pub safety: SafetyCaps,
  pub engine: EngineInfo,
  pub config_hash: String,          // sha256 hex, first 16 displayed
  pub host: HostInfo,
}

pub struct RunSummary {
  pub measurement_start_ms: i64,
  pub measurement_duration_ms: i64,
  pub warmup_s: u64,
  pub total_attempts: u64,
  pub failed_attempts: u64,
  pub http_errors: u64,
  pub assertion_failures: u64,
  pub timeouts: u64,
  pub connection_failures: u64,
  pub iterations: u64,
  pub latency: LatencyStats,
  pub throughput_rps: f64,
  pub bytes_in: u64,
  pub bytes_out: u64,
  pub per_step: Vec<StepSummary>,
  pub degraded: bool,
}

pub struct LatencyStats {
  pub count: u64,
  pub min_ms: f64,
  pub mean_ms: f64,
  pub p50_ms: f64,
  pub p90_ms: f64,
  pub p95_ms: f64,
  pub p99_ms: f64,
  pub max_ms: f64,
}

pub struct ObjectiveResult {
  pub objective_id: Id,
  pub metric: MetricKind,
  pub target: f64,
  pub actual: Option<f64>,
  pub result: ObjectiveOutcome,     // Passed, Failed, InsufficientData, NotEvaluated
  pub samples: u64,
}
```

### 10.3 Validation rules

| Field | Rule |
|---|---|
| Project name | 1–80 chars, trimmed, unique case-insensitive |
| Environment name | 1–60 chars, unique per project |
| Base URL | absolute http/https, no username/password, valid host |
| Variable key | `[A-Za-z_][A-Za-z0-9_]*`, unique per scope, not reserved |
| Header name | token chars, non-empty, no CR/LF |
| Header value | no CR/LF |
| Step URL | non-empty; relative allowed; `{{var}}` allowed; secret vars forbidden |
| Timeout | 100–300,000 ms |
| Think time | 0–60,000 ms; range min ≤ max |
| Status code | 100–599 |
| JSON body | valid JSON when no placeholders; warning when placeholders present |
| Assertion JSON path | dot/bracket syntax only; expected value scalar |
| Regex | compiles |
| VUs | 1–10,000 |
| Duration | 1–86,400 s |
| Stages | 1–20; duration > 0; target 0–10,000 |
| Arrival rate | 1–100,000 per time unit |
| Pre-allocated VUs | 1 ≤ pre ≤ max |
| Warm-up | 0 ≤ warmup < total duration |
| Graceful stop | 0–300 s |
| Objective value | finite; ratios 0–1; latency/throughput > 0 |
| Caps | max VUs 1–10,000; max RPS 1–100,000; max duration 10–86,400 s |
| Baseline | same experiment; completed or aborted run allowed with warning |

Validation returns field paths (for example `steps[2].headers[1].value`) so the UI can focus the exact control.

### 10.4 Snapshot and config hash

- Snapshot at run start, before spawning k6.
- Include project name/id, environment (secrets redacted, fingerprints), scenario, profile, objectives, safety, engine info, host info, BenchLab version.
- Canonical hash: SHA-256 over a stable JSON serialization of `{scenario, profile, objectives, environment_semantic}` with object keys sorted, timestamps/ids removed where they do not affect execution, secrets replaced by fingerprints. Display first 16 hex characters.
- Two runs with the same hash and same engine version are considered reproducible apart from secret rotation and target state.
- Store the hash in `runs.snapshot_json` and as a column for indexing? Store only in JSON; add a `config_hash` column for quick comparison. Yes, add to `runs`.

### 10.5 Profile presets

Illustrative defaults, fully editable:

| Preset | Executor | Shape | Warm-up |
|---|---|---|---|
| Smoke | Constant VUs | 1 VU, 30 s | 0 s |
| Load | Ramping VUs | 60 s → 50 VUs, hold 240 s, 30 s → 0 | 30 s |
| Stress | Ramping VUs | 60 s → 50, 120 s → 150, 120 s → 300, hold 60 s, 60 s → 0 | 60 s |
| Spike | Ramping VUs | 30 s → 20, 10 s → 300, hold 30 s, 10 s → 20, 30 s → 0 | 30 s |
| Soak | Constant VUs | 20 VUs, 60 min | 60 s |
| Custom | Chosen by user | User-defined | User-defined |

Presets are not recommendations. The UI must say: "Starting point only. Set targets from your baseline and requirements."

---

## 11. Persistence

### 11.1 Schemas and migrations

- SQLite via `rusqlite` with bundled SQLite and `tokio-rusqlite` for async access on a dedicated thread.
- On open: `PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;`.
- Migrations are numbered SQL embedded with `include_str!`, applied in a transaction, recorded in `schema_migrations`.
- A failed migration rolls back and blocks startup with the DB path and error.
- Backups before destructive migrations (Phase 2). MVP: no destructive migrations yet.

### 11.2 Initial schema

```sql
CREATE TABLE schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL
);

CREATE TABLE projects (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  accent TEXT NOT NULL DEFAULT 'blue',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT
);
CREATE UNIQUE INDEX idx_projects_name ON projects(lower(name)) WHERE archived_at IS NULL;

CREATE TABLE environments (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('local','staging','production')),
  base_url TEXT NOT NULL,
  variables_json TEXT NOT NULL DEFAULT '[]',
  auth_json TEXT NOT NULL DEFAULT '{"kind":"none"}',
  tls_verify INTEGER NOT NULL DEFAULT 1,
  allow_external_hosts INTEGER NOT NULL DEFAULT 0,
  allowed_hosts_json TEXT NOT NULL DEFAULT '[]',
  caps_json TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(project_id, name)
);
CREATE INDEX idx_environments_project ON environments(project_id);

CREATE TABLE secret_refs (
  id TEXT PRIMARY KEY,
  environment_id TEXT NOT NULL REFERENCES environments(id) ON DELETE CASCADE,
  key TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE(environment_id, key)
);

CREATE TABLE scenarios (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  revision INTEGER NOT NULL DEFAULT 1,
  definition_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  UNIQUE(project_id, name)
);

CREATE TABLE load_profiles (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  revision INTEGER NOT NULL DEFAULT 1,
  definition_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  UNIQUE(project_id, name)
);

CREATE TABLE experiments (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  scenario_id TEXT NOT NULL REFERENCES scenarios(id),
  profile_id TEXT NOT NULL REFERENCES load_profiles(id),
  environment_id TEXT NOT NULL REFERENCES environments(id),
  objectives_json TEXT NOT NULL DEFAULT '[]',
  baseline_run_id TEXT,
  notes TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  UNIQUE(project_id, name)
);
CREATE INDEX idx_experiments_project ON experiments(project_id);

CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  experiment_id TEXT NOT NULL REFERENCES experiments(id) ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN (
    'preflight','starting','running','stopping','finalizing',
    'completed','failed','aborted','interrupted'
  )),
  verdict TEXT CHECK (verdict IN ('passed','failed','inconclusive','aborted','error')),
  trigger TEXT NOT NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  engine_id TEXT NOT NULL,
  engine_version TEXT NOT NULL,
  config_hash TEXT NOT NULL,
  snapshot_json TEXT NOT NULL,
  summary_json TEXT,
  objectives_json TEXT NOT NULL DEFAULT '[]',
  error_message TEXT,
  artifact_dir TEXT NOT NULL,
  host_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);
CREATE INDEX idx_runs_project_started ON runs(project_id, started_at DESC);
CREATE INDEX idx_runs_experiment_started ON runs(experiment_id, started_at DESC);
CREATE INDEX idx_runs_status ON runs(status);

CREATE TABLE run_buckets (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  bucket_ix INTEGER NOT NULL,
  elapsed_ms INTEGER NOT NULL,
  requests INTEGER NOT NULL DEFAULT 0,
  failed INTEGER NOT NULL DEFAULT 0,
  http_errors INTEGER NOT NULL DEFAULT 0,
  assertion_failures INTEGER NOT NULL DEFAULT 0,
  timeouts INTEGER NOT NULL DEFAULT 0,
  conn_failures INTEGER NOT NULL DEFAULT 0,
  iterations INTEGER NOT NULL DEFAULT 0,
  p50_ms REAL,
  p90_ms REAL,
  p95_ms REAL,
  p99_ms REAL,
  mean_ms REAL,
  min_ms REAL,
  max_ms REAL,
  bytes_in INTEGER NOT NULL DEFAULT 0,
  bytes_out INTEGER NOT NULL DEFAULT 0,
  vus INTEGER,
  PRIMARY KEY (run_id, bucket_ix)
);

CREATE TABLE run_steps (
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  step_id TEXT NOT NULL,
  step_name TEXT NOT NULL,
  requests INTEGER NOT NULL DEFAULT 0,
  failed INTEGER NOT NULL DEFAULT 0,
  assertion_failures INTEGER NOT NULL DEFAULT 0,
  timeouts INTEGER NOT NULL DEFAULT 0,
  conn_failures INTEGER NOT NULL DEFAULT 0,
  p50_ms REAL,
  p95_ms REAL,
  p99_ms REAL,
  mean_ms REAL,
  min_ms REAL,
  max_ms REAL,
  PRIMARY KEY (run_id, step_id)
);

CREATE TABLE run_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  elapsed_ms INTEGER NOT NULL,
  level TEXT NOT NULL,
  kind TEXT NOT NULL,
  message TEXT NOT NULL,
  data_json TEXT,
  UNIQUE(run_id, seq)
);

CREATE TABLE app_settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);
```

Notes:

- Timestamps are RFC3339 UTC strings for readability; bucket elapsed is integer milliseconds.
- JSON columns are validated in Rust, not by SQLite.
- `run_buckets` stores null percentiles for buckets with no latency samples.
- `run_steps` stores overall step stats for the measurement window. Per-step time series is Phase 2.
- Phase 2 adds `reports`, `schedules`, `suite_runs`, and AI provider settings.

### 11.3 Repositories

- `ProjectRepo`, `EnvironmentRepo`, `ScenarioRepo`, `ProfileRepo`, `ExperimentRepo`, `RunRepo`, `SettingsRepo`.
- Each exposes typed methods and returns domain errors, not `rusqlite::Error`.
- Writes use transactions where multiple rows change (run finalization, project deletion, baseline update).
- Lists use keyset pagination for runs; other lists are small and load fully.
- Search uses `LIKE` with escaped wildcards; FTS is Phase 2.

### 11.4 Artifacts

```text
<data_dir>/
├── benchlab.db
├── benchlab.lock
├── logs/
│   └── benchlab.YYYY-MM-DD.log
└── runs/
    └── <run_id>/
        ├── snapshot.json
        ├── script.js
        ├── raw.jsonl
        ├── summary.json
        ├── k6.log
        └── engine.json
```

- `engine.json`: `{ pid, port, started_at, k6_version, command_redacted }`.
- Artifact paths are UUID-based; step names never appear in paths.
- `snapshot.json` is the same JSON as `runs.snapshot_json`.
- `script.js` contains no secret values.
- `raw.jsonl` is the k6 JSON output. It may contain URLs and status codes; secrets in URLs are prohibited by validation.
- On deletion, DB rows are deleted first, then the directory. Failures to delete files are logged and shown as a warning.

### 11.5 Data directory

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\BenchLab` (`dirs::data_dir()/BenchLab`) |
| Linux | `~/.local/share/benchlab` |
| macOS | `~/Library/Application Support/BenchLab` |
| Override | `BENCHLAB_DATA_DIR` (absolute path) |

- `BENCHLAB_DATA_DIR` is used by tests and portable mode.
- Logs rotate daily and are retained for 7 days.
- The app shows disk usage and warns at 5 GB of artifacts. Automatic pruning is Phase 2.

### 11.6 Secrets

- Keyring service: `dev.benchlab.app`; account: `secret:<secret_ref_id>`.
- Values are wrapped in `secrecy::SecretString` and zeroized on drop.
- Secret fingerprints: SHA-256 of the value, first 8 hex characters, stored in snapshots to detect rotation.
- If the keychain is unavailable (common in minimal WSL), the UI offers session-only secrets: values live in memory for the app session and are never persisted. Preflight and reports state that secret persistence is unavailable.
- Secrets MUST NOT appear in: SQLite, snapshots, scripts, logs, crash reports, toasts, or exported artifacts.

---

## 12. Engine integration

### 12.1 Engine trait

```rust
pub struct EngineInfo { pub id: String, pub version: String, pub path: PathBuf }

pub struct PreflightCheck {
  pub id: &'static str,
  pub label: String,
  pub result: CheckResult, // Pass, Warn(String), Fail(String)
}

pub struct RunPlan {
  pub run_id: Uuid,
  pub scenario: Scenario,
  pub profile: LoadProfile,
  pub environment: Environment,
  pub objectives: Vec<Objective>,
  pub resolved_secrets: SecretMap,
  pub allowed_hosts: Vec<String>,
  pub artifact_dir: PathBuf,
}

#[async_trait::async_trait]
pub trait LoadEngine: Send + Sync {
  fn info(&self) -> EngineInfo;
  fn preflight(&self, plan: &RunPlan) -> Vec<PreflightCheck>;
  fn prepare(&self, plan: &RunPlan) -> Result<PreparedRun>; // script + args + config
  async fn start(&self, prepared: PreparedRun, events: EngineEventSink) -> Result<EngineHandle>;
}

pub struct EngineHandle {
  pub stop: Box<dyn Fn(StopMode) -> BoxFuture<'static, Result<()>> + Send + Sync>,
  pub wait: Box<dyn FnOnce() -> BoxFuture<'static, EngineExit> + Send>,
}
```

MVP implements only `K6Engine`. A native engine or remote engine is added in Phase 3 without changing `RunService`.

### 12.2 k6 discovery and support

Discovery order:

1. Settings `engine.k6_path`.
2. `BENCHLAB_K6_PATH`.
3. `k6` on `PATH`.
4. Common locations:
   - Windows: `%LOCALAPPDATA%\Microsoft\WinGet\Links\k6.exe`, `C:\Program Files\k6\k6.exe`, `C:\ProgramData\chocolatey\bin\k6.exe`.
   - Linux/macOS: `/usr/local/bin/k6`, `/usr/bin/k6`, `~/.local/bin/k6`.
5. If found, run `k6 version` with a 5 s timeout.

Parse output like `k6 v2.2.0 (commit/..., go1.24.0, windows/amd64)`.

Supported range: `>= 2.0.0, < 3.0.0`. Tested against 2.2.x/2.3.x.
- Unsupported version: preflight fails with the detected version and supported range.
- Unparseable output: preflight fails; Settings shows the raw first line.
- Missing: preflight fails with install guidance. Do not auto-download in the MVP.

Install guidance:

- Windows: `winget install --id GrafanaLabs.k6 -e` (verified package ID; winget currently offers 2.2.0).
- Alternate Windows: `choco install k6`.
- macOS: `brew install k6`.
- Linux: distribution package or official tarball; link to k6 docs.
- After install, a new terminal may be needed for `PATH`; Settings offers **Choose k6 binary** and **Re-detect**.

Engine info is recorded in every run snapshot and report.

### 12.3 Script generation

The generated script is **data-driven**, not string-concatenated code:

- Rust builds a `ScriptConfig` struct and serializes it with `serde_json` into `const CONFIG = {...};`.
- A fixed runtime template implements request execution, assertions, extraction, auth, host checks, and custom metrics.
- User input never becomes executable JavaScript except as JSON data.
- Step IDs, names, headers, bodies, assertions, and variables are data.
- Assertions are implemented as a fixed `switch` over an `AssertionKind` enum.
- No dynamic `eval`, no user-provided JS.

`ScriptConfig`:

```json
{
  "base_url": "http://127.0.0.1:8787",
  "allowed_hosts": ["127.0.0.1:8787"],
  "steps": [
    {
      "id": "0199...",
      "name": "List items",
      "method": "GET",
      "url": "{{base_url}}/items",
      "headers": {"Accept": "application/json"},
      "query": {},
      "body": null,
      "timeout_ms": 5000,
      "expected_status": [200],
      "assertions": [
        {"kind": "status_in", "values": [200], "label": "status 200"},
        {"kind": "max_duration_ms", "value": 800, "label": "under 800 ms"}
      ],
      "extract": [
        {"kind": "json_path", "variable": "first_id", "path": "items[0].id"}
      ],
      "think_time": {"kind": "range", "min_ms": 100, "max_ms": 300}
    }
  ],
  "variables": {"base_url": "http://127.0.0.1:8787"},
  "secret_variables": ["token"],
  "auth": {"kind": "bearer", "token_env": "BENCHLAB_AUTH_TOKEN"},
  "discard_response_bodies": false
}
```

Rules:

- `base_url` is always available as a variable.
- Non-secret variables are embedded in `CONFIG.variables`.
- Secret variables are listed in `secret_variables`; their values come from `__ENV["BENCHLAB_SECRET_<KEY>"]`. Missing values throw during iteration init.
- Auth values always come from `__ENV` variables set by the parent process: `BENCHLAB_AUTH_TOKEN`, `BENCHLAB_AUTH_USERNAME`, `BENCHLAB_AUTH_PASSWORD`, `BENCHLAB_AUTH_VALUE`.
- Step headers override generated auth headers when the same header name is present.
- Secret variable placeholders are rejected in URLs/query at validation time.
- `discard_response_bodies` is true only when no assertion or extraction needs a body. This reduces k6 memory.
- JSON bodies with placeholders are sent as text after substitution. The single-request test is the safety net.

Host pinning runtime guard:

```js
function hostAllowed(rawUrl) {
  try {
    const u = new URL(rawUrl);
    return CONFIG.allowed_hosts.includes(u.host);
  } catch (_) {
    return false;
  }
}
```

If blocked, the step throws, counts as an iteration error, and does not send a request. Preflight also validates static hosts.

### 12.4 k6 invocation

```text
k6 run \
  --no-color \
  --quiet \
  --address 127.0.0.1:<port> \
  --out json=<run_dir>/raw.jsonl \
  --summary-export=<run_dir>/summary.json \
  <run_dir>/script.js
```

- Spawn with an argument array, never a shell string.
- Set the child working directory to the run artifact directory.
- Pass `BENCHLAB_RUN_ID`, `BENCHLAB_BASE_URL`, non-secret variables, secret values, and auth values through `Command::env`.
- Redirect stdout/stderr to `k6.log`. Keep the last 100 lines in memory for error display.
- Set `kill_on_drop(true)` and record the PID.
- Reserve a free localhost port for the REST API by binding `127.0.0.1:0`, recording the port, then dropping the listener. Retry up to three times if k6 fails to bind.
- Poll `GET /v1/status` every 200 ms for up to 15 s to confirm readiness.
- Wait for process exit. Capture exit code.
- `--summary-export` is a cross-check; the internal finalization from raw JSONL is authoritative.

### 12.5 k6 options

Generated `options`:

```json
{
  "scenarios": {
    "benchlab": {
      "executor": "ramping-vus",
      "startVUs": 0,
      "stages": [
        {"duration": "60s", "target": 50},
        {"duration": "240s", "target": 50},
        {"duration": "30s", "target": 0}
      ],
      "gracefulRampDown": "0s",
      "gracefulStop": "30s",
      "tags": {"phase": "measure"}
    }
  },
  "thresholds": {
    "http_req_duration": [
      {"threshold": "p(95)<250", "abortOnFail": true, "delayAbortEval": "30s"}
    ],
    "benchlab_http_errors": ["rate<0.01"],
    "benchlab_requests_total": ["rate>=200"]
  },
  "summaryTrendStats": ["avg", "min", "med", "max", "p(90)", "p(95)", "p(99)"],
  "discardResponseBodies": false,
  "noConnectionReuse": false,
  "maxRedirects": 10,
  "throw": false,
  "userAgent": "BenchLab/0.1.0",
  "tags": {"run_id": "<uuid>"}
}
```

- Thresholds are generated for every objective. `abortOnFail` is set only when the objective has it; use the long form with `delayAbortEval` equal to warm-up.
- k6 thresholds are a backstop and a transparency feature. The internal evaluator is authoritative because k6 evaluates over the whole run, including warm-up, and its standard `http_req_failed` does not know per-step expected statuses.
- Custom metrics:
  - `benchlab_requests_total` Counter, one per attempt.
  - `benchlab_failed_requests` Rate, 1 if any failure.
  - `benchlab_http_errors` Rate, transport or unexpected status.
  - `benchlab_assertion_errors` Rate, any assertion failure.
  - `benchlab_timeouts` Rate.
  - `benchlab_connection_errors` Rate.
  - `benchlab_extraction_failures` Counter.
- Objective-to-metric mapping:
  - Latency metrics → `http_req_duration` percentiles/mean/min/max.
  - Throughput → `benchlab_requests_total` rate.
  - HTTP error rate → `benchlab_http_errors` rate.
  - Combined error rate → `benchlab_failed_requests` rate.
  - Assertion failure rate → `benchlab_assertion_errors` rate.
  - Timeout rate → `benchlab_timeouts` rate.
  - Connection failure rate → `benchlab_connection_errors` rate.
- Per-step threshold tags use `{step:<uuid>}`.

### 12.6 Stop and exit

Stop sequence:

1. Mark the run `stopping`; record `StopReason` (`user_graceful`, `user_force`, `cap_duration`, `cap_rate`, `disk_low`, `objective_abort`, `app_shutdown`).
2. `PATCH http://127.0.0.1:<port>/v1/status` with the documented k6 body setting `stopped: true`.
3. Wait for the child to exit up to `graceful_stop_s + 5 s`.
4. If it has not exited, terminate the process. Wait 2 s. Force kill if necessary.
5. If REST is unavailable, terminate directly and record `degraded_stop`.
6. Finalize from whatever raw data exists.

Exit handling:

| Exit code | Meaning | Action |
|---|---|---|
| 0 | Normal completion | Finalize; evaluate objectives |
| 99 | k6 threshold failure | Finalize; internal verdict authoritative |
| Other | Engine error | Finalize partial metrics if useful; status failed; verdict error |
| Killed | User/cap/watchdog | Status aborted; verdict aborted |
| No samples | Nothing measured | Status failed or aborted depending on reason; verdict error/aborted |

### 12.7 Watchdog

Runs every 250 ms while active:

- Child process alive; if not, transition to finalizing.
- Elapsed time ≤ `min(profile total + graceful_stop, caps.max_duration_s)`. Stop at cap with `cap_duration`.
- 5-second rolling RPS ≤ `caps.max_rps` for VU-based profiles. If exceeded, stop with `cap_rate`. Arrival-rate profiles are validated against the cap before start.
- Free disk ≥ 200 MB; otherwise stop with `disk_low`.
- Optional objective abort: if an `abort_on_fail` error-rate objective has failed for three consecutive post-warm-up buckets, stop with `objective_abort`.
- Generator RSS warning at 1.5 GB; log only. Do not stop on memory in the MVP.
- If the UI has not consumed a snapshot for 5 s, log a warning; do not stop the run. The engine is independent.

Resource sampling:

- `sysinfo` reads k6 PID CPU and RSS once per second.
- Values are shown in the live view and stored in run events every 5 s, not in every bucket.
- Labeled "load generator (this machine)", never "target".

### 12.8 JSONL parsing

- Tail `raw.jsonl` with a 250 ms poll. Read new bytes, keep a partial-line buffer, parse complete lines as JSON.
- Handle `{"type":"Metric",...}` definitions by recording metric names and ignoring for aggregation.
- Handle `{"type":"Point","data":{"time":"...","value":...,"tags":{...}},"metric":"..."}`.
- Parse timestamps with `chrono`; compute `elapsed_ms = point.time - first_point.time`.
- Bucket index = `elapsed_ms / 1000` (floor).
- Aggregate per bucket:
  - `http_req_duration`: HDR histogram (3 significant figures) → p50/p90/p95/p99/mean/min/max; count.
  - `benchlab_requests_total`: sum values → requests.
  - `benchlab_failed_requests`, `benchlab_http_errors`, `benchlab_assertion_errors`, `benchlab_timeouts`, `benchlab_connection_errors`: sum values (each point is 0/1) → counts.
  - `benchlab_extraction_failures`: sum.
  - `iterations`: sum.
  - `vus`: last value in bucket.
  - `data_received`, `data_sent`: sum.
  - Per-step tags `step`: maintain per-step counters and HDR histograms.
- Unknown metrics are ignored for aggregation but preserved in raw.
- A malformed line increments a parse-error counter, logs once per 100 errors, and does not abort the run.
- Parser lag: if the file's last modified time is more than 3 s behind wall clock while running, mark `degraded` and show an amber badge. Do not stop the run.
- At completion, read remaining bytes and parse the final partial line if newline-terminated.

### 12.9 Finalization

1. Wait for child exit.
2. Re-read `raw.jsonl` from the beginning (streaming) to compute exact final statistics after warm-up:
   - Exact nearest-rank percentiles.
   - Per-step stats.
   - Total attempts and failure flags.
   - Throughput denominator: `last_sample_time - max(first_sample_time, warmup_end)`, with a 1 s minimum.
3. If re-read fails, use live HDR aggregates and set `degraded=true`.
4. Evaluate objectives.
5. Generate findings.
6. Persist summary, objective results, per-step rows, and final status/verdict in one transaction.
7. Emit a completion event to the UI.
8. Keep raw artifacts for inspection.

### 12.10 Failure classification

Classify each attempt from the k6 response object:

- `timeout`: `res.error` contains `timeout` (case-insensitive).
- `connection`: error contains one of `connection refused`, `connection reset`, `no such host`, `dns`, `dial`, `tls`, `handshake`, `eof`.
- `http`: `step.expected_status` does not contain `res.status`, or any transport error not classified as timeout/connection.
- `assertion`: at least one `check` returned false.
- `failed`: any of the above.
- `extraction`: a variable extraction returned null/undefined or threw.

The generated JS implements this once; the Rust aggregator consumes the custom Rate metrics. The Rust and JS classifications must be tested against the same fixtures.

Common error codes are recorded from tags for the error stream, but the classification is based on message/category, not a fragile code table. Unknown errors are classified as `http` (transport) and shown as "Other".

---

## 13. Metrics and statistics

### 13.1 Definitions

| Metric | Definition |
|---|---|
| Total attempts | One per `http.request` call, including transport failures. Source: `benchlab_requests_total`. |
| Latency sample | A completed HTTP response duration (`http_req_duration`). Transport failures without a response may have no duration. |
| HTTP error | Unexpected status or transport error, excluding assertion-only failures. |
| Assertion failure | At least one check for the step returned false. |
| Failed attempt | Any HTTP error, timeout, connection failure, or assertion failure. Flags may overlap. |
| Error rate | Failed attempts / total attempts. |
| Throughput | Total attempts / measurement duration. |
| Measurement duration | Last sample time − max(first sample time, warm-up end), minimum 1 s. |
| Active VUs | Last `vus` gauge value in the bucket. |

### 13.2 Percentiles

- Final percentiles: exact nearest-rank.
- Given sorted values `v[0..n)`, percentile `p` (0–100): `index = ceil(p/100 * n) - 1`, clamped to `[0, n-1]`.
- p50 on `[1,2,3,4]` = 2; p95 on 20 values = value at index 18.
- Live percentiles: HDR histogram with 3 significant figures; used for charts only.
- Display `n` next to every percentile. Below 30 samples, mark insufficient for objectives.
- Never label a percentile as an average. Mean is shown separately and labeled "mean".

### 13.3 Warm-up

- `warmup_s` is a prefix of the run, measured from the first sample.
- Samples with `elapsed_ms < warmup_s * 1000` are excluded from objectives and summary statistics.
- Warm-up samples remain in charts and are shaded.
- Warm-up may overlap ramp-up. The UI states this and shows both on the timeline.
- If the run ends before warm-up, all objectives are `insufficient_data`; verdict is `inconclusive` or `aborted` if stopped.

### 13.4 Throughput and errors

- Throughput uses total attempts, not completed responses. A connection failure still consumed capacity and is part of attempted load.
- Error rate is a ratio from 0 to 1; UI displays percent with one decimal for values under 10%, otherwise whole percent.
- The report always shows the numerator and denominator: "12 failed / 84,213 attempts (0.014%)".
- Timeout and connection failures are subsets of failed attempts; they may overlap with HTTP errors. The UI labels them "attempts with timeout" and "attempts with connection failure" rather than implying disjoint counts.
- Latency percentiles include responses with unexpected status if a duration exists. This is intentional: slow failures matter. The report offers a "successful only" note in Phase 2; not a toggle in MVP.

### 13.5 Objective evaluation

For each objective:

1. Require at least 30 relevant samples/attempts in the measurement window. If not, result = `insufficient_data`.
2. Compute the metric.
3. Compare with the operator and target:
   - Latency/error: actual ≤ target (Lte) or < target (Lt).
   - Throughput: actual ≥ target (Gte) or > target (Gt).
4. Result: `passed` or `failed`.
5. Margin = target − actual for upper bounds (positive means under target); actual − target for lower bounds (positive means above target). Display with unit.
6. If the run is aborted, results are `not_evaluated` unless the measurement window was complete before the stop. In practice, aborted runs show partial metrics and `not_evaluated` objectives.

Verdict aggregation:

- If engine/preflight error → `error`.
- Else if aborted → `aborted`.
- Else if any objective failed → `failed`.
- Else if any objective insufficient → `inconclusive`.
- Else if zero objectives → completed with label "Completed, no objectives".
- Else → `passed`.

### 13.6 Comparison deltas

For runs A (reference) and B:

- Absolute delta = `B − A`.
- Percent delta = `(B − A) / A × 100` when A ≠ 0; display "—" otherwise.
- Error rates: percentage-point delta = `(B_rate − A_rate) × 100`.
- Throughput: percent delta only, plus absolute.
- Direction labels: for latency/errors, lower is better; for throughput, higher is better. Use text + icon, not color alone.
- Sample counts are always shown next to each value.
- No significance test in MVP. The compare screen states this and includes the sentence: "Small differences may be noise, especially with fewer than 1,000 samples or short measurement windows."
- Compatibility warnings are based on config hash, executor kind, environment class, and engine version.

---

## 14. Offline findings

Findings are deterministic, rule-based, and always available. They are not AI.

| ID | Condition | Severity | Message pattern |
|---|---|---|---|
| F-01 | Any objective failed | Critical | "p99 was 538 ms against a 500 ms target (38 ms over, 84,213 samples)." |
| F-02 | p99 / p50 > 10 and p99 > 100 ms | Warning | "Tail latency is high relative to the median. p99 is N× p50, consistent with a subset of slow requests." |
| F-03 | Throughput drops >20% and p95 rises >25% in the last third vs the first third of the measurement window | Warning | "Throughput fell while latency rose in the final third, consistent with saturation or a degrading dependency." |
| F-04 | ≥3 consecutive buckets with error rate > max(1%, 3× run median) | Critical | "Errors clustered from 02:10 to 02:14 (12.4% at peak). Inspect the error breakdown for that window." |
| F-05 | A step's p95 > 2× the median step p95 and has ≥50 samples | Warning | "Step 'Checkout' is N× slower than the median step." |
| F-06 | Warm-up p95 > 1.5× measurement p95 | Info | "Warm-up latency was higher than measured latency. Cache/JIT effects may explain the difference." |
| F-07 | Any objective has <30 samples | Info | "p99 has only 28 samples. Treat the value as indicative, not conclusive." |
| F-08 | Timeout rate > 0.5% | Warning | "N attempts timed out (X%). Inspect timeouts in the per-step table." |
| F-09 | Connection failure rate > 0 | Warning | "N attempts failed to connect. Check DNS, TLS, port, and connection limits." |
| F-10 | Run duration < 30 s and ≥30 samples | Info | "Short runs can miss slow-tail behavior. Consider a longer measurement window for tail metrics." |
| F-11 | Baseline exists and p95 changed ≥20% | Warning | "p95 changed from A to B (C%) versus the baseline." |
| F-12 | Baseline exists and error rate changed ≥1 percentage point | Warning | "HTTP error rate changed from A% to B% versus the baseline." |

Rules:

- Maximum six findings shown, ordered by severity, then by rule ID.
- Every finding has `evidence_refs`; clicking a chip navigates to the objective, chart, bucket window, or step.
- Text uses observation language: "consistent with", "may indicate", "inspect". Never "the database is the cause".
- No finding without evidence. If no rule matches, show the empty state.
- Findings are unit-tested with fixture runs.

Evidence model:

```rust
pub struct Finding {
  pub rule_id: &'static str,
  pub severity: Severity,       // Info, Warning, Critical
  pub title: String,
  pub detail: String,
  pub evidence: Vec<EvidenceRef>,
}

pub enum EvidenceRef {
  Objective { id: Uuid },
  Metric { name: String },
  Bucket { metric: String, start_ix: u32, end_ix: u32 },
  Step { step_id: Uuid, metric: String },
  Run { run_id: Uuid },
  Config { field: String },
}
```

---

## 15. AI analysis (Phase 2)

This section defines the contract so the MVP data model and report UI do not need rework. It is not implemented in the MVP.

### 15.1 Principles

- Opt-in per run. No automatic uploads.
- Evidence-only prompting. The model receives structured measurements, configuration, and findings, never screenshots or raw bodies.
- Redaction before transmission: no secrets, headers, cookies, request/response bodies, or query values. Hosts masked by default; paths retained.
- Structured JSON output validated against a schema. Every claim must cite evidence IDs from the input.
- Deterministic findings remain available without AI.

### 15.2 Provider

- OpenAI-compatible chat completions endpoint (`POST {base_url}/chat/completions`).
- Settings: base URL, model, API key (keychain), temperature (default 0.2), timeout (default 60 s), max output tokens.
- Local providers (Ollama, LM Studio) work through the same interface, with no key.
- TLS required for non-localhost base URLs; HTTP allowed only for localhost with a warning.
- Test connection action; store provider metadata, never log the key.
- One retry on 429/5xx with exponential backoff (2 s, 8 s).

### 15.3 Evidence pack

```json
{
  "schema_version": 1,
  "run": {
    "id": "<short>",
    "verdict": "failed",
    "started_at": "...",
    "duration_s": 330,
    "warmup_s": 30,
    "measurement_window_s": 300,
    "engine": "k6 2.2.0",
    "environment_class": "staging",
    "target_host": "<target>",
    "scenario": {"name": "...", "revision": 3, "steps": [...]},
    "load_profile": {"kind": "load", "executor": "ramping_vus", "max_vus": 50, "warmup_s": 30},
    "objectives": [{"id": "obj_1", "metric": "latency_p99", "target": 500, "actual": 538, "result": "failed", "samples": 84213}],
    "summary": {"throughput_rps": 286.4, "http_error_rate": 0.004, "timeout_rate": 0.0, "p50_ms": 92, "p95_ms": 184, "p99_ms": 538},
    "buckets": [{"t": 30, "rps": 280, "p95": 180, "p99": 510, "errors": 3}],
    "steps": [{"name": "...", "requests": 12000, "p95_ms": 210, "error_rate": 0.002}],
    "findings": [{"id": "F-01", "severity": "critical", "title": "...", "detail": "..."}],
    "baseline": {"run_id": "...", "p95_delta_pct": 18.0, "error_delta_pp": 0.2}
  },
  "evidence": [
    {"id": "ev_1", "kind": "objective", "label": "p99 objective", "value": "538 ms vs 500 ms target"},
    {"id": "ev_2", "kind": "bucket_window", "label": "02:10–02:14", "value": "p99 512–690 ms"}
  ]
}
```

- Buckets are downsampled to at most 300 points.
- Strings are truncated to safe lengths.
- No headers, bodies, cookies, auth, or secret values.
- Hosts default to `<target>`; users can opt in to include the host.
- Payload preview in the UI before sending.

### 15.4 Output schema

```json
{
  "summary": "string",
  "verdict_commentary": "string",
  "anomalies": [
    {
      "title": "string",
      "observation": "string",
      "evidence_ids": ["ev_1"],
      "confidence": "low|medium|high",
      "caveats": "string"
    }
  ],
  "comparisons": [
    {
      "statement": "string",
      "baseline_run_id": "string|null",
      "evidence_ids": ["ev_2"]
    }
  ],
  "hypotheses": [
    {
      "statement": "string",
      "confidence": "low|medium|high",
      "supporting_evidence_ids": ["ev_1"],
      "caveats": "string"
    }
  ],
  "recommended_investigations": [
    {
      "action": "string",
      "rationale": "string"
    }
  ],
  "limitations": ["string"]
}
```

Validation:

- Reject unknown evidence IDs; do not display unsupported claims.
- Require at least one evidence ID for each anomaly, comparison, and hypothesis.
- Cap arrays: 6 anomalies, 4 comparisons, 5 hypotheses, 8 recommendations.
- If validation fails once, retry with a repair instruction; if it fails again, show the deterministic report and an error message.

### 15.5 Prompt

System prompt (versioned in code):

> You are a performance engineer analyzing an API load test. Use only the evidence provided. Separate observations from hypotheses. Do not assert causation. Cite evidence IDs for every claim. State limitations, including sample size, warm-up, and configuration differences. If the evidence is insufficient, say so. Return only JSON matching the provided schema.

User prompt: the evidence pack, the output schema, and the question "Explain the result and recommend the next investigations."

Store `prompt_version`, `prompt_hash`, provider, model, created_at, token usage if available, and the raw response. No secrets.

### 15.6 UI

- Report section "AI analysis" below findings.
- If no provider: "Connect an AI provider to generate an explanation. Deterministic findings above are always available." Link to Settings.
- Button: **Analyze with AI**. Shows provider/model and "Preview payload".
- Loading: progress with cancel; 60 s timeout.
- Result: summary, anomalies, hypotheses, recommended investigations, limitations. Evidence chips link to the report.
- Label: "Generated by <model>. Evidence-based, not a diagnosis."
- Actions: Regenerate, Copy as Markdown.
- Errors: inline banner with retry; never hide the deterministic report.
- No AI content is presented as measured fact. Numeric values are always taken from the evidence pack, not from model text.

---

## 16. Automation (Phase 2 and 3)

Not in the MVP. The following contracts are reserved.

### 16.1 Schedules

- Cron expression, timezone, enabled flag, next run time.
- The app must be running; no background daemon in Phase 2.
- Missed runs are skipped, not caught up.
- A schedule references an experiment and optional baseline.
- Each scheduled run records trigger `schedule`.
- Desktop notification on failure; optional webhook.
- Safety caps still apply. Production schedules require an additional per-schedule confirmation.

### 16.2 Suites

- Ordered list of experiments.
- Stop on failure or continue options.
- Baseline comparison per experiment.
- Suite run groups member runs and produces a summary verdict.
- Machine-readable suite result JSON.

### 16.3 CLI and CI

```text
benchlab run --experiment <id> --json [--fail-on-objective] [--data-dir <path>]
```

Exit codes:

| Code | Meaning |
|---|---|
| 0 | Passed |
| 1 | Objective failed |
| 2 | Error (preflight, engine, config) |
| 3 | Inconclusive |
| 4 | Aborted |

- JSON output includes run ID, verdict, objectives, summary, artifact paths, and engine version.
- CLI uses the same database and data directory as the GUI, with the single-instance lock; CI uses a separate `--data-dir`.
- No secrets in CLI arguments; environment/keychain resolution only.
- Phase 3: remote generators and a server API.

---

## 17. Testing

### 17.1 Test pyramid

| Layer | Scope | Tools |
|---|---|---|
| Unit | Metric math, validation, script generation, objectives, findings, hashing | `cargo test`, `insta` |
| Storage | Migrations, repositories, snapshots, deletion, baseline updates | in-memory/temp SQLite |
| Parser | Recorded k6 JSONL fixtures, malformed lines, tags, warm-up, steps | fixtures |
| Engine integration | Real k6 against local mock target, stop, caps, crash | `#[ignore]` unless `BENCHLAB_K6_TESTS=1` |
| End-to-end | Create demo project, run, report, compare | app service APIs, headless |
| UI | View models, validation, keyboard actions, route transitions | GPUI test context |
| Manual | Visual quality, DPI, resize, long run memory | checklist |

### 17.2 Unit test requirements

- Percentile nearest-rank on known vectors; p50/p95/p99.
- Warm-up exclusion at boundaries (t = warmup ± 1 ms).
- Bucket assignment at 999/1000/1001 ms.
- Throughput denominator with 0, 1, and many samples.
- Error classification for timeout, connection, TLS, DNS, unexpected status, assertion.
- Objective evaluation: pass, fail, equal-to-target, insufficient samples, aborted.
- Verdict aggregation for all combinations.
- Comparison deltas, zero baseline, percentage points.
- Config hash stability and sensitivity.
- Validation: every rule, field paths, reserved variables, secret-in-URL rejection.
- Script generation: snapshot for a three-step scenario; no secret values; JSON escaping; host allowlist; auth header construction.
- Findings: one fixture per rule, plus no-finding case.
- Retention/deletion: DB rows and artifacts.

### 17.3 k6 fixtures

- `tests/fixtures/k6/raw-small.jsonl`: 120 requests over 10 buckets, one step, a few errors.
- `tests/fixtures/k6/raw-multistep.jsonl`: two steps, extraction tags, timeouts.
- `tests/fixtures/k6/raw-warmup.jsonl`: samples before and after 30 s.
- `tests/fixtures/k6/summary.json`: matching summary export.
- `tests/fixtures/k6/malformed.jsonl`: truncated line, invalid JSON, unknown metric.
- Parser and finalization tests must use these without k6 installed.
- Record fixtures with a documented k6 version; parser must tolerate additional fields.

### 17.4 Mock target

A local `axum` server used by the demo and integration tests.

| Endpoint | Behavior |
|---|---|
| `GET /health` | 200 `{"ok":true}` |
| `GET /items` | 200 JSON list of 20 items |
| `GET /items/:id` | 200 item; `id=0` → 404 |
| `POST /echo` | 200, echoes JSON body |
| `GET /slow?ms=250` | Sleeps the given ms (cap 10,000), 200 |
| `GET /status/:code` | Returns that status code |
| `GET /flaky?seed=1&fail=0.1` | Deterministic pseudo-random 500s based on seed |
| `GET /bytes/:n` | Returns n bytes of `x` (cap 1 MB) |
| `POST /reset` | Resets counters |
| `GET /stats` | JSON request count, per-path counts, last headers |
| `GET /slow-tail` | 95% fast, 5% slow (seeded), for tail tests |

- Binds `127.0.0.1:0` and prints the port for tests.
- Demo project seeds the port at creation time; if the server restarts on a different port, the demo re-seeds or updates the environment base URL.
- No external network required.

### 17.5 Engine integration tests

Enabled with `BENCHLAB_K6_TESTS=1` and a real k6 binary.

1. **Smoke pass**: 1 VU, 5 s, `/health`; assert status completed, verdict passed, samples > 0, artifacts exist, DB rows.
2. **Objective fail**: p95 target 1 ms against `/slow?ms=50`; assert failed, margin negative.
3. **Warm-up**: 2 s warm-up, 3 s measurement; assert summary samples exclude warm-up.
4. **Stop**: start 60 s run, stop after 2 s; assert aborted, child exits, partial artifacts retained.
5. **Force stop**: disable REST (simulate by wrong port) and stop; assert kill and degraded stop.
6. **Missing k6**: configure an invalid path; assert preflight blocks and no child spawns.
7. **Crash**: kill the k6 child; assert failed/error and stderr excerpt.
8. **Secret redaction**: run with a secret header; assert the value is absent from DB, snapshot, script, raw JSONL, and logs.
9. **Host pinning**: step points to a disallowed host; assert blocked and no request reaches `/stats`.
10. **Cap enforcement**: arrival rate above `max_rps`; assert validation blocks. VU-based RPS cap: mock fast endpoint, low cap; assert watchdog aborts.

### 17.6 End-to-end tests

- Seed a temp data dir and demo project.
- Run through services (no GPUI window): create experiment, preflight, start, wait, finalize, read report model.
- Assert verdict, objectives, charts data non-empty, findings, snapshot hash.
- Create a second run with the same config and compare; assert deltas are zero within tolerance.
- Delete the run; assert rows and directory are gone.
- Simulate app restart with an active run; assert recovery.

### 17.7 UI tests

- GPUI `TestAppContext` for view models:
  - Scenario validation shows the correct field path.
  - Objective editor restricts operators by metric.
  - Experiment Run button disabled with reasons.
  - Command palette filters actions and entities.
  - Route transitions preserve project context.
  - Dirty-form guard.
- No pixel-perfect snapshot testing in the MVP. Visual QA is manual.
- Keyboard-only pass through the primary flow.

### 17.8 Manual QA checklist

- Windows 100%, 125%, 150%, 200% scaling.
- Minimum window 1080×720; resize during live run.
- 5-minute run with 50 VUs; UI remains responsive; no frame >32 ms while interacting.
- 1-hour soak; memory stable within 10%.
- 0-sample/failed-target run.
- Missing k6; unsupported k6.
- Keychain available and unavailable.
- Production confirmation and caps.
- Delete flows with confirmation.
- Command palette and keyboard focus.
- Report with failed/inconclusive/aborted verdicts.
- Compare incompatible runs.
- App crash recovery.

### 17.9 CI

- GitHub Actions matrix: `windows-latest`, `ubuntu-latest`.
- Steps: checkout, Rust 1.96 toolchain, cache, Linux GPUI dependencies, install k6 (pinned), `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, release build.
- `BENCHLAB_K6_TESTS=1` on both platforms.
- Artifacts: failed test logs, fixture diffs.
- macOS job optional and non-blocking.
- No network tests against public targets.

---

## 18. Performance budgets

| Budget | Target |
|---|---|
| Cold start to interactive shell | < 1.5 s on the reference machine (warm OS cache) |
| Route switch | < 50 ms |
| Live UI frame time | < 16 ms typical, no frame > 32 ms during interaction |
| Chart paint | < 8 ms at 1440 px width, 1-hour data |
| Metric freshness | ≤ 2 s from sample to screen |
| Run start | < 2 s from confirmation to k6 spawn, excluding preflight network |
| DB batch write | < 50 ms |
| Memory, UI + services | < 300 MB with 1-hour run, 1 s buckets |
| Memory, live aggregation | Streaming; no per-sample retention in memory beyond the current bucket and step histograms |
| Supported load envelope | Up to 3,000 requests/s and 300 VUs on a 12-core desktop; warn above this |
| Raw artifact size | Estimate shown in preflight; roughly 150–300 bytes per sample |
| Idle CPU | < 1% |
| Log file | Rotate daily, retain 7 days |
| Data warning | 5 GB artifacts; no automatic deletion in MVP |

Reference machine: 12 logical cores, 7.6 GB RAM (the development WSL host). Document that results depend on the machine and that the generator competes with the target if both run locally.

Parser lag handling:

- If lag > 3 s, mark degraded and show a badge.
- If lag > 10 s, show "Live metrics are delayed. The test continues."
- Never drop raw data; persistence may lag but finalization re-reads the file.
- Above the supported envelope, show a preflight warning based on configured VUs/RPS.

---

## 19. Security and privacy

### 19.1 Threat model

| Threat | Mitigation |
|---|---|
| Secret leakage into artifacts/logs | Keychain storage; env passing; validation; redaction tests |
| Command injection through scenario fields | No shell; arg arrays; generated JS is data-driven; no user JS |
| Accidental load against a third party | Host pinning; authorization checkbox; production confirmation; caps |
| Runaway test | Max duration, VU/RPS caps, watchdog, stop control, disk guard |
| Malicious OpenAPI (Phase 2) | Parse only; no code execution; validation; preview |
| AI data leakage (Phase 2) | Opt-in, redaction, payload preview, no bodies/headers |
| Local database access | User-profile permissions; secrets not in DB; document that the DB is not encrypted |
| k6 binary tampering | User-selected path; show version; no auto-download in MVP |
| Orphaned load process | PID file, command-line marker, startup cleanup |
| Crash log leakage | No request headers/bodies/secrets in logs; review redaction |

### 19.2 Secrets

- OS keychain via `keyring`; secrets wrapped in `SecretString`.
- No plaintext fallback. If the keychain is unavailable, session-only secrets with a visible warning.
- Secret fingerprints in snapshots for rotation detection.
- Secret placeholders banned in URLs/query.
- Report and snapshot show only `••••••` + fingerprint.
- "Copy k6 command" uses placeholders.

### 19.3 Redaction

- Log filter redacts header names: `authorization`, `cookie`, `set-cookie`, `x-api-key`, `proxy-authorization`.
- Query parameters matching `token|key|secret|password|auth|sig|signature` are masked in displayed URLs.
- Request/response bodies are never logged by BenchLab. The single-request test displays them locally and does not persist them.
- Error messages from k6 are scanned for known secret values before display; if found, replace with `[redacted]` and log a redaction warning.

### 19.4 Target safety

- Environment class drives defaults:
  - Local: max 500 VUs, no default RPS cap, max 60 min, no typed confirmation.
  - Staging: max 100 VUs, max 500 RPS, max 15 min, authorization checkbox.
  - Production: max 20 VUs, max 100 RPS, max 5 min, typed confirmation + authorization checkbox.
- Caps are editable by the user but defaults are conservative. The UI warns when raising production caps.
- Host pinning defaults to base host only.
- Preflight sends one request; the confirmation text mentions it.
- No automatic retries of a stopped run.

### 19.5 Privacy

- No telemetry, analytics, crash reporting, or update checks in the MVP.
- All data stays in the data directory.
- AI is Phase 2 and opt-in per run.
- Export (Phase 2) includes a redaction preview.

---

## 20. Build, run, package

### 20.1 Prerequisites

**Windows (primary)**

- Windows 10/11 x64.
- Rust 1.96.0 MSVC (installed; `rustc 1.96.0`, `cargo 1.96.0`).
- Visual Studio 2022 Build Tools with the C++ workload (installed at `C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools`).
- k6 2.x. Install:
  ```powershell
  winget install --id GrafanaLabs.k6 -e
  ```
  or
  ```powershell
  choco install k6
  ```
  After installation, open a new terminal or set the k6 path in Settings.
- WebView2 is not required; GPUI renders natively.

**Linux (secondary)**

- Rust 1.96.0.
- Build dependencies for GPUI and graphics; consult the GPUI crate documentation for the current list. Typical Debian/Ubuntu packages:
  ```bash
  sudo apt install build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev \
    libwayland-dev libx11-xcb-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev \
    libxcb-xfixes0-dev libfontconfig1-dev libvulkan1 mesa-vulkan-drivers
  ```
- WSLg for running the GUI under WSL. Build in the Linux filesystem (for example `~/benchlab`), not `/mnt/c`, to avoid slow I/O and path issues.
- k6:
  ```bash
  sudo apt install k6   # if available in the distribution
  # or download the official tarball
  ```

**macOS (best-effort)**

- Rust 1.96.0; Xcode command line tools; `brew install k6`. Not acceptance-tested.

### 20.2 Commands

From a Windows terminal at the repository root:

```powershell
cd C:\Users\mehdi\Documents\ai\BenchLab
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p benchlab-app
```

From WSL/OpenCode, invoke Windows Cargo explicitly:

```bash
powershell.exe -NoProfile -Command "Set-Location 'C:\Users\mehdi\Documents\ai\BenchLab'; cargo run -p benchlab-app"
```

Do not attempt to build the Windows target with a WSL Cargo that does not exist. For a Linux build, use a WSL Rust toolchain and build under the Linux filesystem.

Environment variables:

| Variable | Purpose |
|---|---|
| `BENCHLAB_DATA_DIR` | Override data directory |
| `BENCHLAB_K6_PATH` | Override k6 binary |
| `BENCHLAB_LOG` | `info` (default), `debug`, `trace` |
| `BENCHLAB_K6_TESTS` | `1` enables integration tests that need k6 |
| `RUST_LOG` | Optional tracing filter override |

### 20.3 First build checklist

1. `cargo build -p benchlab-app` succeeds on Windows.
2. Window opens with the shell and theme.
3. A text input accepts typing, paste, selection, and IME.
4. A canvas chart renders 300 fake points at 60 fps.
5. k6 is detected or the engine warning is shown.
6. The demo target starts and responds on localhost.
7. The demo run completes and opens a report.
8. Restart preserves the project and run history.
9. No secret appears in the data directory.
10. `cargo test` passes without k6; `BENCHLAB_K6_TESTS=1 cargo test` passes with k6.

### 20.4 Packaging

- MVP: `cargo build --release -p benchlab-app`. The binary embeds fonts, icons, and demo assets.
- Windows release output: `target\release\benchlab.exe`.
- No installer in the MVP. A portable ZIP with the binary and a README is acceptable.
- k6 is not bundled. Document the external dependency and license.
- Phase 2: MSI or portable installer, code signing, version metadata, auto-update (opt-in).
- macOS/Linux packaging is Phase 2/3.

### 20.5 Licenses

- GPUI: Apache-2.0.
- Inter: SIL OFL 1.1.
- JetBrains Mono: SIL OFL 1.1.
- Lucide icons: ISC.
- k6: AGPL-3.0, external process, not linked. BenchLab does not bundle the k6 binary. Include an acknowledgement in About and the README.
- Include a `THIRD_PARTY_NOTICES.md` generated or maintained manually.

---

## 21. Implementation plan

Estimates are for one experienced Rust developer with AI assistance, assuming the GPUI spike succeeds. They exclude design exploration beyond the defined system.

### M0 — Platform spike (0.5–1 day)

- Create the workspace and `rust-toolchain.toml`.
- Add GPUI 0.2.2, open a window on Windows.
- Render the shell skeleton, one text input, and one canvas chart with 300 fake points.
- Verify GPU rendering, DPI scaling, and frame time.
- Verify k6 detection on Windows.
- **Gate A:** window, typing, chart, and k6 detection work on Windows. If GPUI Windows is blocked, pin the latest known-good Zed git revision or fall back to Linux/WSLg; do not fork GPUI.

### M1 — Foundation (2–3 days)

- Theme tokens and core components: Button, TextInput, Select, Table, Modal, Toast, Badge, Banner.
- SQLite migrations, repositories, app paths, logging, single-instance lock.
- App shell, routing, project switcher, welcome, project overview.
- Project and environment CRUD, including secrets and caps.
- **Gate B:** create a project and environment, restart, and see persisted data.

### M2 — Scenario editor (3–4 days)

- Step list and inspector, headers/query/body editors, assertions, extraction, think time.
- Variables panel and validation with field paths.
- Single-request test and redacted response viewer.
- Mock target and demo seed.
- **Gate C:** build a three-step scenario with extraction and test each step against the mock target.

### M3 — Profiles, objectives, experiments (2–3 days)

- Profile presets and custom executor editor, timeline, derived values.
- Objective editor with validation.
- Experiment editor, baseline selection, sticky summary.
- Preflight sheet UI (checks wired in M4).
- **Gate D:** create a complete experiment with caps and objectives; Run is disabled with a reason until valid.

### M4 — Engine (4–5 days)

- Script generator with fixtures and no-secret tests.
- k6 discovery, version parsing, preflight.
- Supervisor, REST stop, watchdog, artifacts, env passing.
- JSONL tailer, parser, aggregator, per-step stats.
- **Gate E:** headless service test runs a 10-second scenario against the mock target, stops on demand, and produces correct artifacts and metrics.

### M5 — Live run (2–3 days)

- RunService state machine, DB writes, recovery.
- Live view readouts, chart, objectives, error stream, log, generator stats.
- Stop/force stop UI.
- **Gate F:** a 5-minute run updates the UI smoothly; stop works; restart recovery works.

### M6 — Report and compare (3–4 days)

- Exact finalization, objective evaluation, verdict.
- Report layout, charts, per-step table, findings, configuration, artifacts.
- Compare selection, compatibility, overlays, delta table, baseline action.
- **Gate G:** demo project runs end-to-end and produces a report with a meaningful finding.

### M7 — Hardening (3–4 days)

- Secret redaction tests, host pinning tests, cap tests.
- Empty/loading/error states, keyboard navigation, focus order.
- Performance pass: chart decimation, DB batching, memory.
- Crash recovery, orphan cleanup, unsupported-engine paths.
- Acceptance checklist, README, build instructions.
- **Gate H:** all acceptance tests in Appendix E pass.

**Total:** roughly 20–25 working days, plus 20% buffer.

### Definition of done (MVP)

- A new user can create a project and run a test against the demo target without reading documentation.
- A real user can test a local or staging HTTP API with auth, assertions, extraction, and a load profile.
- Every run has a verdict, sample counts, artifacts, and a configuration snapshot.
- Live metrics are useful and never block the UI.
- Stop, caps, production confirmation, and host pinning work.
- Secrets never appear in the data directory.
- Reports are understandable without reading the raw JSONL.
- Comparison works for two to four compatible runs.
- Crash recovery works.
- Windows acceptance passes; Linux builds and tests pass.
- `cargo fmt`, `cargo clippy -D warnings`, and `cargo test` pass.

---

## 22. Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| GPUI 0.2.x API churn / Windows defects | Medium | High | M0 spike; pin exact version; isolate UI crate; fallback to known-good git revision; no fork |
| Custom text input takes too long | Medium | High | Time-box; one reusable entity; fallback to a third-party primitive behind the internal API |
| Custom charts exceed time budget | Medium | Medium | Start with one time-series chart, then histogram/sparkline; no chart crate; decimation from day one |
| k6 not installed or PATH issues | High | Medium | Guided setup, detected path, Settings picker, clear preflight error, CI installs k6 |
| k6 version drift / JSON format changes | Medium | Medium | Supported range; parser tolerates unknown fields; golden fixtures; record version |
| Statistics incorrect or misleading | Medium | High | Exact final percentiles; warm-up tests; sample counts; no significance claims; fixture tests |
| Parser cannot keep up at high RPS | Medium | Medium | 1 s aggregation, documented envelope, degraded badge, raw file authoritative |
| Scope creep into AI/schedules/OpenAPI | High | High | Phase gates; no dead UI; data model reserved only |
| `/mnt/c` build performance | High | Low | Build with Windows Cargo on NTFS; Linux builds in ext4 |
| Keychain unavailable in WSL | Medium | Low | Session-only secrets with visible warning; Windows primary |
| Secret leakage | Low | Critical | Keychain, env passing, validation, redaction tests, no bodies in logs |
| Accidental load against third party | Low | Critical | Host pinning, authorization, caps, production confirmation |
| Orphaned k6 after crash | Medium | High | PID/port file, command-line marker, startup cleanup, kill |
| Run data size grows unbounded | Medium | Medium | Size estimates, disk warning, manual deletion, retention Phase 2 |
| Target and generator compete on the same machine | High | Medium | Label generator metrics; document; recommend testing remote targets; distributed generation Phase 3 |
| User expects AI in MVP | Medium | Low | Deterministic findings provide value; AI is Phase 2 and clearly scoped |

---

## 23. Open questions

1. **macOS acceptance:** Is a macOS build required for release, or is best-effort compilation sufficient? Current assumption: best-effort, not acceptance-tested.
2. **k6 distribution:** Confirm that k6 stays external (no bundled binary). Current decision: external, due to license, size, and update cadence.
3. **Native engine timing:** Is a pure Rust engine a Phase 3 differentiator, or should it replace k6 earlier? Current decision: Phase 3.
4. **Timeline and team size:** Estimates assume one experienced Rust developer with AI assistance. Adjust milestones if the team differs.
5. **Demo target lifecycle:** Should the local mock server run only during demo/tests, or be available as a permanent "Local demo" environment? Current decision: started on demand and referenced by the demo project.
6. **Report export:** HTML/Markdown export is Phase 2. Is a JSON export needed in the MVP for power users? Current decision: raw artifacts and snapshot are sufficient.
7. **Retention:** Keep all runs by default with a 5 GB warning. Confirm whether automatic pruning should be earlier.
8. **Data directory portability:** Is portable mode (`BENCHLAB_DATA_DIR` on a USB drive) a requirement? Current decision: supported by env var, not a UI feature.
9. **Production targets:** Is typed confirmation plus authorization sufficient, or is a second approver required? Current decision: single-user typed confirmation.
10. **Sample size policy:** Is 30 samples the right minimum for objectives? Current decision: 30, configurable only in code for MVP.

---

## Appendix A: Example run snapshot

```json
{
  "schema_version": 1,
  "benchlab_version": "0.1.0",
  "created_at": "2026-10-20T18:42:11Z",
  "project": {"id": "0199...", "name": "Inventory API"},
  "environment": {
    "id": "0199...",
    "name": "Local",
    "kind": "local",
    "base_url": "http://127.0.0.1:8787",
    "tls_verify": true,
    "allow_external_hosts": false,
    "allowed_hosts": ["127.0.0.1:8787"],
    "caps": {"max_vus": 500, "max_rps": null, "max_duration_s": 3600, "require_confirmation": false},
    "variables": [
      {"key": "base_url", "value": "http://127.0.0.1:8787", "secret": false},
      {"key": "token", "value": null, "secret": true, "fingerprint": "9f2c1a7b"}
    ],
    "auth": {"kind": "bearer", "token_var": "token"}
  },
  "scenario": {
    "id": "0199...",
    "name": "Inventory smoke",
    "revision": 3,
    "variables": [],
    "steps": [
      {
        "id": "0199...",
        "name": "List items",
        "method": "GET",
        "url": "{{base_url}}/items",
        "headers": {"Accept": "application/json"},
        "query": [],
        "body": {"kind": "none"},
        "timeout_ms": 5000,
        "think_time": {"kind": "range", "min_ms": 100, "max_ms": 300},
        "expected_status": [200],
        "assertions": [
          {"kind": "status_in", "values": [200], "label": "status 200"},
          {"kind": "max_duration_ms", "value": 800, "label": "under 800 ms"}
        ],
        "extract": [
          {"kind": "json_path", "variable": "first_id", "path": "items[0].id"}
        ]
      },
      {
        "id": "0199...",
        "name": "Get item",
        "method": "GET",
        "url": "{{base_url}}/items/{{first_id}}",
        "headers": {"Accept": "application/json"},
        "query": [],
        "body": {"kind": "none"},
        "timeout_ms": 5000,
        "think_time": {"kind": "fixed", "ms": 150},
        "expected_status": [200],
        "assertions": [{"kind": "json_path_equals", "path": "id", "value": 1, "label": "id matches"}],
        "extract": []
      }
    ]
  },
  "profile": {
    "id": "0199...",
    "name": "Load 50",
    "kind": "load",
    "revision": 1,
    "executor": {
      "kind": "ramping_vus",
      "stages": [
        {"duration_s": 60, "target_vus": 50},
        {"duration_s": 240, "target_vus": 50},
        {"duration_s": 30, "target_vus": 0}
      ]
    },
    "warmup_s": 30,
    "graceful_stop_s": 30
  },
  "objectives": [
    {"id": "obj1", "metric": "latency_p95", "operator": "lte", "value": 250, "abort_on_fail": false},
    {"id": "obj2", "metric": "latency_p99", "operator": "lte", "value": 500, "abort_on_fail": false},
    {"id": "obj3", "metric": "http_error_rate", "operator": "lt", "value": 0.01, "abort_on_fail": true}
  ],
  "safety": {"max_vus": 500, "max_rps": null, "max_duration_s": 3600, "require_confirmation": false},
  "engine": {"id": "k6", "version": "2.2.0", "path": "C:\\Users\\mehdi\\AppData\\Local\\Microsoft\\WinGet\\Links\\k6.exe"},
  "config_hash": "4f8c1d2a9b7e3c60",
  "host": {"os": "windows", "arch": "x86_64", "hostname": "desktop", "cpus": 12, "memory_bytes": 8160437862}
}
```

Secrets are never included. The fingerprint detects rotation; it is not reversible.

---

## Appendix B: Example generated k6 script

```js
import http from 'k6/http';
import { check, sleep } from 'k6';
import { Counter, Rate } from 'k6/metrics';
import encoding from 'k6/encoding';

const CONFIG = {
  base_url: "http://127.0.0.1:8787",
  allowed_hosts: ["127.0.0.1:8787"],
  variables: { base_url: "http://127.0.0.1:8787" },
  secret_variables: ["token"],
  auth: { kind: "bearer", token_env: "BENCHLAB_AUTH_TOKEN" },
  discard_response_bodies: false,
  steps: [
    {
      id: "0199a1",
      name: "List items",
      method: "GET",
      url: "{{base_url}}/items",
      headers: { Accept: "application/json" },
      body: null,
      timeout_ms: 5000,
      expected_status: [200],
      assertions: [
        { kind: "status_in", values: [200], label: "status 200" },
        { kind: "max_duration_ms", value: 800, label: "under 800 ms" }
      ],
      extract: [{ kind: "json_path", variable: "first_id", path: "items[0].id" }],
      think_time: { kind: "range", min_ms: 100, max_ms: 300 }
    },
    {
      id: "0199a2",
      name: "Get item",
      method: "GET",
      url: "{{base_url}}/items/{{first_id}}",
      headers: { Accept: "application/json" },
      body: null,
      timeout_ms: 5000,
      expected_status: [200],
      assertions: [
        { kind: "json_path_equals", path: "id", value: 1, label: "id matches" }
      ],
      extract: [],
      think_time: { kind: "fixed", ms: 150 }
    }
  ]
};

export const options = {
  scenarios: {
    benchlab: {
      executor: "ramping-vus",
      startVUs: 0,
      stages: [
        { duration: "60s", target: 50 },
        { duration: "240s", target: 50 },
        { duration: "30s", target: 0 }
      ],
      gracefulRampDown: "0s",
      gracefulStop: "30s"
    }
  },
  thresholds: {
    "http_req_duration": [
      { threshold: "p(95)<250", abortOnFail: false, delayAbortEval: "30s" },
      { threshold: "p(99)<500", abortOnFail: false, delayAbortEval: "30s" }
    ],
    "benchlab_http_errors": [
      { threshold: "rate<0.01", abortOnFail: true, delayAbortEval: "30s" }
    ]
  },
  summaryTrendStats: ["avg", "min", "med", "max", "p(90)", "p(95)", "p(99)"],
  discardResponseBodies: false,
  noConnectionReuse: false,
  maxRedirects: 10,
  throw: false,
  userAgent: "BenchLab/0.1.0",
  tags: { run_id: __ENV.BENCHLAB_RUN_ID }
};

const requestsTotal = new Counter('benchlab_requests_total');
const failedRequests = new Rate('benchlab_failed_requests');
const httpErrors = new Rate('benchlab_http_errors');
const assertionErrors = new Rate('benchlab_assertion_errors');
const timeouts = new Rate('benchlab_timeouts');
const connectionErrors = new Rate('benchlab_connection_errors');
const extractionFailures = new Counter('benchlab_extraction_failures');

function render(text, vars) {
  return String(text).replace(/\{\{([A-Za-z_][A-Za-z0-9_]*)\}\}/g, (_, key) => {
    if (!(key in vars)) {
      throw new Error('Missing variable: ' + key);
    }
    return String(vars[key]);
  });
}

function getPath(value, path) {
  const parts = String(path).replace(/\[(\d+)\]/g, '.$1').split('.').filter(Boolean);
  let current = value;
  for (const part of parts) {
    if (current === null || current === undefined) return undefined;
    current = current[part];
  }
  return current;
}

function hostAllowed(rawUrl) {
  try {
    return CONFIG.allowed_hosts.includes(new URL(rawUrl).host);
  } catch (_) {
    return false;
  }
}

function authHeaders() {
  const a = CONFIG.auth;
  if (a.kind === 'bearer') {
    return { Authorization: 'Bearer ' + __ENV[a.token_env] };
  }
  if (a.kind === 'basic') {
    const raw = __ENV[a.username_env] + ':' + __ENV[a.password_env];
    return { Authorization: 'Basic ' + encoding.b64encode(raw) };
  }
  if (a.kind === 'header') {
    const headers = {};
    headers[a.name] = __ENV[a.value_env];
    return headers;
  }
  return {};
}

function classify(res) {
  const error = String(res.error || '').toLowerCase();
  const timeout = error.includes('timeout');
  const connection = /(connection refused|connection reset|no such host|dns|dial|tls|handshake|eof)/.test(error);
  return { timeout, connection };
}

function assertionPasses(a, res) {
  try {
    switch (a.kind) {
      case 'status_in':
        return a.values.includes(res.status);
      case 'header_equals':
        return String(res.headers[a.name] || '') === String(a.value);
      case 'body_contains':
        return String(res.body || '').includes(String(a.value));
      case 'body_regex':
        return new RegExp(a.pattern).test(String(res.body || ''));
      case 'json_path_equals': {
        const actual = getPath(JSON.parse(String(res.body || 'null')), a.path);
        return JSON.stringify(actual) === JSON.stringify(a.value);
      }
      case 'max_duration_ms':
        return res.timings.duration <= a.value;
      case 'max_size_bytes':
        return String(res.body || '').length <= a.value;
      default:
        return false;
    }
  } catch (_) {
    return false;
  }
}

function extractValue(ex, res) {
  try {
    switch (ex.kind) {
      case 'json_path':
        return getPath(JSON.parse(String(res.body || 'null')), ex.path);
      case 'regex': {
        const match = new RegExp(ex.pattern).exec(String(res.body || ''));
        return match ? match[ex.group] : undefined;
      }
      case 'header':
        return res.headers[ex.name];
      case 'status':
        return res.status;
      default:
        return undefined;
    }
  } catch (_) {
    return undefined;
  }
}

export default function () {
  const vars = Object.assign({}, CONFIG.variables);
  for (const key of CONFIG.secret_variables) {
    const value = __ENV['BENCHLAB_SECRET_' + key.toUpperCase()];
    if (value === undefined) {
      throw new Error('Missing secret variable: ' + key);
    }
    vars[key] = value;
  }

  const baseAuth = authHeaders();

  for (const step of CONFIG.steps) {
    const url = render(step.url, vars);
    if (!hostAllowed(url)) {
      throw new Error('Blocked host: ' + url);
    }

    const headers = Object.assign({}, baseAuth);
    for (const [name, value] of Object.entries(step.headers || {})) {
      headers[name] = render(value, vars);
    }

    const body = step.body === null || step.body === undefined ? null : render(step.body, vars);
    requestsTotal.add(1);

    const res = http.request(step.method, url, body, {
      headers,
      timeout: step.timeout_ms + 'ms',
      tags: { step: step.id, step_name: step.name },
      redirects: 10
    });

    const kinds = classify(res);
    const unexpectedStatus = !step.expected_status.includes(res.status);
    const httpFailure = unexpectedStatus || (!!res.error && !kinds.timeout && !kinds.connection) || kinds.connection;

    let assertionsOk = true;
    for (const a of step.assertions || []) {
      const passed = check(
        res,
        { [step.name + ': ' + a.label]: (r) => assertionPasses(a, r) },
        { step: step.id }
      );
      if (!passed) assertionsOk = false;
    }

    httpErrors.add(httpFailure);
    timeouts.add(kinds.timeout);
    connectionErrors.add(kinds.connection);
    assertionErrors.add(!assertionsOk);
    failedRequests.add(httpFailure || kinds.timeout || kinds.connection || !assertionsOk);

    for (const ex of step.extract || []) {
      const value = extractValue(ex, res);
      if (value === undefined || value === null) {
        extractionFailures.add(1);
      } else {
        vars[ex.variable] = value;
      }
    }

    const think = step.think_time || { kind: 'none' };
    if (think.kind === 'fixed') {
      sleep(think.ms / 1000);
    } else if (think.kind === 'range') {
      sleep((think.min_ms + Math.random() * (think.max_ms - think.min_ms)) / 1000);
    }
  }
}
```

Notes:

- Secret environment variable names are sanitized to uppercase with non-alphanumerics replaced by `_`.
- Auth env vars are set by Rust; values never appear in this file.
- The script is stored for inspection. It is valid JavaScript; `CONFIG` is JSON data.
- `__ENV.BENCHLAB_RUN_ID` is set by the parent process.

---

## Appendix C: Metric mapping and formulas

### C.1 k6 to BenchLab

| k6 metric | Type | BenchLab use |
|---|---|---|
| `http_req_duration` | Trend | Latency samples, percentiles, per-step latency |
| `http_reqs` | Counter | Cross-check total attempts |
| `http_req_failed` | Rate | Cross-check failure classification |
| `vus` | Gauge | Active VUs |
| `iterations` | Counter | Completed iterations |
| `data_received` | Counter | Bytes in |
| `data_sent` | Counter | Bytes out |
| `checks` | Rate | Cross-check assertions |
| `benchlab_requests_total` | Counter | Authoritative total attempts |
| `benchlab_failed_requests` | Rate | Any failure |
| `benchlab_http_errors` | Rate | Unexpected status or transport error |
| `benchlab_assertion_errors` | Rate | Assertion failure |
| `benchlab_timeouts` | Rate | Timeout |
| `benchlab_connection_errors` | Rate | Connection/DNS/TLS failure |
| `benchlab_extraction_failures` | Counter | Extraction failures |

### C.2 Tags

| Tag | Source | Use |
|---|---|---|
| `step` | Generated params | Per-step aggregation |
| `step_name` | Generated params | Display only |
| `status` | k6 | Error breakdown |
| `error` | k6 | Classification |
| `error_code` | k6 | Error stream detail |
| `run_id` | Global options | Correlation |
| `phase` | Scenario options | Reserved |

### C.3 Formulas

```text
total_attempts      = Σ benchlab_requests_total points (value)
failed_attempts     = Σ benchlab_failed_requests points (value)
http_errors         = Σ benchlab_http_errors points (value)
assertion_failures  = Σ benchlab_assertion_errors points (value)
timeouts            = Σ benchlab_timeouts points (value)
conn_failures       = Σ benchlab_connection_errors points (value)
error_rate          = failed_attempts / total_attempts
http_error_rate     = http_errors / total_attempts
throughput_rps      = total_attempts / measurement_duration_s
measurement_start   = max(first_sample_time, run_start + warmup)
measurement_end     = last_sample_time
p(k)                = sorted_latencies[clamp(ceil(k/100 * n) - 1, 0, n - 1)]
```

Warm-up filtering:

```text
include sample iff elapsed_ms >= warmup_s * 1000
```

Bucket assignment:

```text
bucket_ix = floor(elapsed_ms / 1000)
```

### C.4 Verdict matrix

| Condition | Verdict |
|---|---|
| Preflight/engine error | error |
| Stop reason present | aborted |
| Any objective failed | failed |
| Any objective insufficient and none failed | inconclusive |
| All objectives passed | passed |
| No objectives | completed (no objective verdict) |

### C.5 Units and formatting

- Latency: ms below 1,000; s above, one decimal. Always show the unit.
- Throughput: `req/s`; `k req/s` above 10,000, one decimal.
- Ratios: percent, one decimal below 10%, whole percent above. Show numerator/denominator in the report.
- Bytes: B, KB, MB, GB with one decimal.
- Durations: `m:ss` under 1 hour; `h:mm:ss` above.
- Percentiles: label `p95`, never "95th percentile average".

---

## Appendix D: Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+K` | Command palette |
| `Ctrl+N` | New experiment |
| `Ctrl+Shift+N` | New project |
| `Ctrl+S` | Save current form |
| `Ctrl+Enter` | Run experiment (from experiment editor) |
| `Ctrl+Shift+R` | Re-run last experiment |
| `Esc` | Close overlay / cancel edit |
| `↑` / `↓` | Move selection in lists, palette, select |
| `Enter` | Open selection / confirm |
| `Space` | Toggle focused checkbox/switch |
| `Tab` / `Shift+Tab` | Move focus |
| `Alt+←` | Back to project overview |
| `Ctrl+,` | Settings |
| `?` | Shortcut help overlay (MVP: static list) |

Rules:

- No single-key destructive shortcut.
- Stop run is click-only in the MVP to avoid accidental stops. `Esc` does not stop a run; it closes overlays.
- Shortcuts are disabled while a modal is open, except `Esc`.
- Shortcut hints use `Ctrl` on Windows/Linux; the UI can display `Cmd` on macOS.

---

## Appendix E: Acceptance checklist

### E.1 Project and environment

- [ ] Create a project; it persists across restart.
- [ ] Duplicate project names are rejected case-insensitively.
- [ ] Create local, staging, and production environments.
- [ ] Save a secret; verify it is absent from SQLite, snapshots, logs, and artifacts; verify it is present in the OS keychain.
- [ ] Disable the keychain; verify session-only secrets warning and behavior.
- [ ] Invalid base URLs are rejected with an inline message.
- [ ] Production run requires typing the environment name and checking authorization.
- [ ] A step pointing to a disallowed external host is blocked at runtime and no request is sent.

### E.2 Scenario

- [ ] Create a three-step scenario with headers, JSON body, timeout, and think time.
- [ ] Assertions and extraction validate and run.
- [ ] Secret placeholders in URLs are rejected.
- [ ] Single-request test shows status, duration, size, headers, and body; secrets are masked.
- [ ] Reordering steps changes execution order in the next run.
- [ ] Editing a scenario after a run does not change the historical report.

### E.3 Load and objectives

- [ ] Applying each preset produces a valid profile with a correct timeline.
- [ ] Custom VU and arrival-rate profiles validate against caps.
- [ ] Required VUs above the cap block save/run.
- [ ] Objectives reject incompatible operators and non-finite values.
- [ ] Default objectives do not include a fabricated throughput target.
- [ ] Minimum sample rule produces inconclusive, not passed.

### E.4 Run

- [ ] Preflight blocks on missing k6, unreachable target, missing secret, and cap violation.
- [ ] Start writes snapshot, script, engine metadata, and raw artifacts.
- [ ] Live metrics appear within two seconds; charts use one-second buckets.
- [ ] Stop gracefully ends the run within the graceful period; verdict is aborted.
- [ ] Force stop kills the child and retains partial artifacts.
- [ ] Max duration cap stops the run.
- [ ] Restart during a run recovers the run and kills the orphan.
- [ ] Only one run can be active.
- [ ] Generator CPU/RSS are labeled as local generator metrics.

### E.5 Report

- [ ] Verdict appears before metrics.
- [ ] Objectives show target, actual, result, sample count, and margin.
- [ ] Latency chart includes objective lines and warm-up shading.
- [ ] Throughput, error breakdown, and histogram render with correct units.
- [ ] Per-step table matches fixture expectations.
- [ ] Findings cite evidence and never claim causation.
- [ ] Configuration snapshot redacts secrets and shows engine version.
- [ ] Copy k6 command contains placeholders, not secrets.
- [ ] Failed, inconclusive, aborted, and error verdicts all render correctly.
- [ ] Zero-sample run shows an honest empty state.

### E.6 Compare

- [ ] Select two to four runs; overlay charts share an elapsed axis.
- [ ] Delta table shows absolute and percent change with sample counts.
- [ ] Incompatible runs show a warning but still render.
- [ ] Set as baseline updates the experiment.
- [ ] The no-significance note is visible.

### E.7 Demo and settings

- [ ] Demo project is created once and is runnable without external network.
- [ ] Demo run completes with a meaningful report.
- [ ] Engine detection shows path/version; invalid path gives an actionable error.
- [ ] Settings opens the data folder and shows disk usage.
- [ ] About shows version and third-party notices.
- [ ] No telemetry network calls occur during a session (verified with a proxy/log).

### E.8 Quality

- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo test --workspace` passes without k6.
- [ ] `BENCHLAB_K6_TESTS=1 cargo test --workspace` passes with k6.
- [ ] Linux CI build passes.
- [ ] Manual QA checklist in §17.8 passes.
- [ ] No panic or unhandled error in a 30-minute smoke session.
- [ ] Memory stays within budget during a one-hour run.
- [ ] Accessibility pass: keyboard-only primary flow, visible focus, contrast, non-color verdict encoding.
