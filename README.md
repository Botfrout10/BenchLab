# BenchLab

BenchLab is a native desktop workbench for designing, running, and comparing API
load tests. It keeps raw k6 evidence, computes objectives deterministically, and
shows the latency, throughput, and correctness story without hiding uncertainty.

The product brief is in [Brainstorm.md](Brainstorm.md); the implementation source
of truth is [SPEC.md](SPEC.md).

## Status

**M2 — scenario editor complete.** BenchLab now has scenario CRUD, the ordered
step editor, variables and validation, single-request testing with masked
responses, a deterministic local mock target, and an idempotent demo project.
Gate C evidence is in [docs/m2-report.md](docs/m2-report.md).

M1 established the persistent foundation: projects, environments, variables,
auth, safety caps, settings, the shell, and OS keychain-backed secrets. See
[docs/m1-report.md](docs/m1-report.md) for Gate B evidence, and
[docs/m0-report.md](docs/m0-report.md) for the initial GPUI/k6 platform spike.

## Requirements

- Windows 10/11 x64 with Visual Studio 2022 Build Tools (Desktop development with C++)
  - Linux is supported; macOS is best-effort and untested.
- Rust 1.96.0 (pinned by `rust-toolchain.toml`)
- k6 2.x on `PATH` or in a standard install location
- Windows SDK with `fxc.exe` for **release** builds (GPUI compiles HLSL shaders
  only when debug assertions are off)

BenchLab does **not** bundle k6. Install it separately:

```powershell
winget install --id GrafanaLabs.k6 -e
```

## Run

Build and run from Windows (GPUI's Windows backend is the primary target):

```powershell
cargo run -p benchlab-app
```

Check the engine without opening a window:

```powershell
cargo run -p benchlab-app -- --check
```

The check exits with status 0 when a supported k6 is available. Set
`BENCHLAB_K6_PATH` to override discovery.

BenchLab writes its workspace to the platform application-data directory:
`%APPDATA%\BenchLab` on Windows, `~/Library/Application Support/BenchLab` on
macOS, and `~/.local/share/benchlab` on Linux. Set `BENCHLAB_DATA_DIR` to an
absolute path to keep a separate data set for testing:

```powershell
$env:BENCHLAB_DATA_DIR = "C:\path\to\benchlab-data"
cargo run -p benchlab-app
```

The directory contains `benchlab.db`, logs, run artifacts, and the
single-instance lock. Secret values are never written there; they live in the OS
keychain, or only in memory when no keychain is available (shown in the status
bar).

### Release builds

GPUI's build script needs `fxc.exe` from the Windows SDK when compiling in
release mode. If it is not on `PATH`, set `GPUI_FXC_PATH` before building:

```powershell
$env:GPUI_FXC_PATH = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\fxc.exe"
cargo build --release -p benchlab-app
```

Adjust the SDK version to match the installed Windows SDK. Debug builds do not
need `fxc.exe`.

Measure the repaint interval in the status bar:

```powershell
$env:BENCHLAB_FRAME_METER = "1"
cargo run -p benchlab-app
```

## Workspace

| Crate | Responsibility |
| --- | --- |
| `benchlab-core` | Domain types, validation, metric definitions |
| `benchlab-store` | SQLite persistence, migrations, artifacts, secrets |
| `benchlab-engine` | k6 discovery, script generation, supervision, parsing |
| `benchlab-analysis` | Objectives, final statistics, comparison, findings |
| `benchlab-ui` | GPUI theme, components, views, charts |
| `benchlab-app` | Desktop entry point and window wiring |

The UI never generates load. k6 runs as a separate process and the raw JSONL
stream is the authoritative artifact.

## Quality gates

Run these from the workspace root:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Third-party notes

- **k6** is licensed under AGPL-3.0. BenchLab invokes it as an external process
  and does not link or bundle it. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- GPUI is pinned to exactly `0.2.2` while its API is still moving.

## License

Apache-2.0. See [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Platform note

Editing from WSL is fine, but build and run from Windows. The Windows toolchain
and GPUI backend live outside the WSL environment.
