# M2 scenario editor report

Date: 2026-10-21
Status: **Gate C passed (automated)**

## Scope

M2 adds the scenario authoring and single-request testing surface:

- Scenario CRUD in the project workspace: create, open, duplicate, archive,
  restore, and typed-name deletion.
- Scenario editor with a header, validation badge, environment selector, save,
  unsaved-change guard, and revision display.
- Ordered step list with method chips, paths, assertion/extraction counts, issue
  badges, add/insert-after/duplicate/move/delete actions, and selected-step
  inspector.
- Inspector fields for name, method, URL, timeout, expected statuses, and think
  time (none, fixed, random range).
- Header and query key/value rows, request bodies (none, JSON, raw text) with a
  multiline editor and JSON formatting, assertions, and extractions.
- Scenario variables panel with editable scenario values and read-only, masked
  environment values.
- Field-path validation surfaced in the header badge, affected step card, and
  inspector fields. Invalid drafts remain saveable; runs will stay blocked until
  valid.
- Single-request tester for any step against the selected environment, including
  variable/auth resolution, masked request preview, status, duration, size,
  response headers, formatted body, 256 KB truncation notice, assertion results,
  extraction results, and transient extracted values for subsequent tests.
- Deterministic local mock target and idempotent demo project seed.
- Native Windows UI wiring, including a welcome-screen demo entry point.

## Gate C evidence

Gate C requires a three-step scenario with extraction tested step by step against
the mock target. The automated integration test
`gate_c_demo_three_step_scenario_runs_against_mock_target` opens the bundled demo,
runs each stored step in order through the same `test_request::execute` path used
by the editor, and forwards each response's extracted values into the next
request.

```powershell
cargo test -p benchlab-app gate_c -- --nocapture
```

| Step | Request | Result | Extracted |
| --- | --- | --- | --- |
| 1. List items | `GET {{base_url}}/items` | HTTP 200; status, body, and duration assertions passed | `item_id = 1`, `item_name = Item 1` |
| 2. Fetch item | `GET {{base_url}}/items/{{item_id}}` | HTTP 200; JSON-path and `content-type` assertions passed | `fetched_name = Item 1` |
| 3. Echo item | `POST {{base_url}}/echo` | HTTP 200; body regex and body-contains assertions passed | `echo_status = 200` |

The test also verifies that the final echoed body contains the demo marker
`"source": "benchlab-demo"`. This exercises extraction handoff, variable
interpolation, request bodies, response assertions, and the mock target in one
path.

The mock target is deterministic and bound to `127.0.0.1`. It is started lazily
by `MockTargetManager`, so the demo needs no separate server process.

## Verification

Run from the workspace root on Windows:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p benchlab-app
```

All checks passed at the time of this report (`47` tests). The workspace test
suite covers domain validation, scenario persistence and revision increments,
mock endpoints, single-request masking and truncation, demo idempotency, the
Gate C three-step flow, migrations, project/environment repositories, and secret
storage.

## Deliberate M2 boundaries

- The native editor was compiled and wired on Windows, but this run did not
  repeat an interactive UI smoke test. Gate C is evidenced by the integration
  test above; the remaining verification is a manual click-through on Windows.
- Scenario saves validate static fields, but running a scenario still belongs to
  the engine/run milestones. Invalid drafts are intentionally saveable.
- Secret scenario variables remain unsupported; secrets live on environments.
  The editor displays environment secret values as masked read-only rows.
- `Select` and row actions remain mouse-driven. Keyboard-accessible reordering is
  still outstanding from FR-SCN-06.
- The mock target and demo experiment are M2 surfaces; the demo experiment itself
  is not runnable until the engine and run milestones land.
