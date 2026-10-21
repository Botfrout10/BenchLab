# M0 platform spike report

Date: 2026-10-20
Status: **Gate A passed**

## Scope

M0 proves that the chosen stack can carry the product before foundation work
begins:

- Workspace and pinned Rust toolchain
- GPUI 0.2.2 window on Windows
- Shell skeleton with navigation, readouts, and status
- One reusable text input with selection, clipboard, and IME hooks
- One canvas chart with 300 synthetic points
- k6 detection and version validation

## Environment

| Item | Value |
| --- | --- |
| OS | Windows 11 x64 (WSL2 development shell) |
| GPU | Intel UHD Graphics / NVIDIA RTX 4060 Laptop |
| Display | 1920×1080, 144 Hz, 120% scaling |
| Rust | 1.96.0, MSVC |
| GPUI | 0.2.2 |
| k6 | 2.2.0 at `C:\Program Files\k6\k6.exe` |

## Gate A evidence

| Check | Result |
| --- | --- |
| Window | 1240×760 logical window opens, renders, and is responsive |
| GPU rendering | GPUI's Windows Direct3D renderer painted the shell and canvas chart |
| Text input | Placeholder, focus ring, typing, selection, Home/End, Backspace verified |
| Canvas chart | 300-point p50/p95/p99 series with warm-up band and 250 ms target line |
| k6 detection | `benchlab --check` prints `k6 2.2.0 ready` and exits 0 |
| DPI scaling | Window captured at 1503×955 physical px at 120% scaling |
| Frame time | Release: ~26.6 ms/frame (~38 fps); debug: ~26.8 ms/frame (~37 fps) |
| Quality | `cargo fmt --check`, Clippy `-D warnings`, and tests pass |

Frame time was measured with a continuous repaint enabled by
`BENCHLAB_FRAME_METER=1`. Debug and release produced nearly identical intervals,
which suggests the measurement is dominated by this session's presentation
cadence rather than BenchLab's render code. This is a smoke-test baseline, not a
release performance claim; the M5 chart work will profile real data.

Release builds required `GPUI_FXC_PATH` because GPUI compiles its HLSL shaders
only when debug assertions are off, and its build script probes for a specific
Windows SDK version. Debug builds do not need `fxc.exe`.

## Deliberate deferrals

- The chart has no y-axis labels or tooltips; those belong to the M5 chart.
- Buttons and navigation are visual only; routing and actions start in M1.
- Clipboard and IME handlers are wired into the text input, but a manual IME
  pass with a language-specific keyboard is deferred to M1.
- Native Windows titlebar appearance is left to the OS. A custom titlebar can
  be evaluated in M1 if it improves contrast or branding.
- Content scrolls at 760 logical px height; the shell does not yet have
  responsive breakpoints.

## Decision

Proceed with GPUI 0.2.2 and the workspace layout as specified. No fork or
platform fallback is needed.
