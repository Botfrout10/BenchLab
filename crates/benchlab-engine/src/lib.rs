//! Load-engine discovery and supervision.
//!
//! M0 implements k6 discovery and version validation. The `LoadEngine` trait,
//! script generator, process supervisor, and JSONL aggregator follow in M4.

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

pub use benchlab_core::EngineStatus;

pub const MIN_SUPPORTED_MAJOR: u32 = 2;
pub const MAX_SUPPORTED_MAJOR: u32 = 3;

/// Whether a k6 major version is inside the range BenchLab supports.
pub fn is_supported_major(major: u32) -> bool {
    (MIN_SUPPORTED_MAJOR..MAX_SUPPORTED_MAJOR).contains(&major)
}

/// Locate k6 and verify that it is in the supported major-version range.
///
/// Search order matches SPEC.md §12.2: configured path, environment override,
/// common install locations, then `PATH`.
pub fn detect_k6() -> EngineStatus {
    let mut candidates = Vec::new();

    if let Some(path) = env::var_os("BENCHLAB_K6_PATH") {
        candidates.push(PathBuf::from(path));
    }

    candidates.extend(common_locations());

    if let Some(paths) = env::var_os("PATH") {
        let executable = if cfg!(windows) { "k6.exe" } else { "k6" };
        candidates.extend(env::split_paths(&paths).map(|dir| dir.join(executable)));
    }

    // Let Command resolve a bare name through PATH as a final fallback.
    candidates.push(PathBuf::from(if cfg!(windows) { "k6.exe" } else { "k6" }));

    let mut seen = std::collections::HashSet::new();
    for candidate in candidates {
        if !seen.insert(candidate.clone()) {
            continue;
        }
        if candidate.is_absolute() && !candidate.is_file() {
            continue;
        }
        if let Some(status) = probe(&candidate) {
            return status;
        }
    }

    EngineStatus {
        found: false,
        path: None,
        version: None,
        supported: false,
        message: if cfg!(windows) {
            "k6 not found. Install it with: winget install --id GrafanaLabs.k6 -e".to_string()
        } else {
            "k6 not found. Install it from https://grafana.com/docs/k6/latest/set-up/install-k6/"
                .to_string()
        },
    }
}

fn common_locations() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(windows)]
    {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            paths.push(
                PathBuf::from(local_app_data)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links")
                    .join("k6.exe"),
            );
        }
        if let Some(program_files) = env::var_os("ProgramFiles") {
            paths.push(PathBuf::from(program_files).join("k6").join("k6.exe"));
        }
        if let Some(program_data) = env::var_os("ProgramData") {
            paths.push(
                PathBuf::from(program_data)
                    .join("chocolatey")
                    .join("bin")
                    .join("k6.exe"),
            );
        }
    }

    #[cfg(not(windows))]
    {
        paths.push(PathBuf::from("/usr/local/bin/k6"));
        paths.push(PathBuf::from("/usr/bin/k6"));
        if let Some(home) = env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".local").join("bin").join("k6"));
        }
    }

    paths
}

fn probe(path: &Path) -> Option<EngineStatus> {
    let output = command_for(path).arg("version").output().ok()?;

    let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    }

    let parsed = parse_version(&text);
    let version = parsed
        .as_ref()
        .map(|(major, minor, patch, _)| format!("{major}.{minor}.{patch}"))
        .or_else(|| Some(raw_first_line(&text)));
    let supported = parsed
        .as_ref()
        .is_some_and(|(major, _, _, _)| is_supported_major(*major));

    let message = if parsed.is_none() {
        "k6 was found but its version could not be parsed".to_string()
    } else if supported {
        format!(
            "k6 {} ready",
            version.as_deref().unwrap_or("(unknown version)")
        )
    } else {
        format!(
            "k6 {} is outside the supported range ({}.x)",
            version.as_deref().unwrap_or("(unknown version)"),
            MIN_SUPPORTED_MAJOR
        )
    };

    Some(EngineStatus {
        found: true,
        path: Some(path.to_path_buf()),
        version,
        supported,
        message,
    })
}

#[cfg(windows)]
fn command_for(path: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new(path);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(windows))]
fn command_for(path: &Path) -> Command {
    Command::new(path)
}

fn parse_version(text: &str) -> Option<(u32, u32, u32, String)> {
    for token in text.split_whitespace() {
        let token = token.trim_matches(|c: char| matches!(c, ',' | ')' | '('));
        if let Some(rest) = token.strip_prefix('v') {
            let core = rest.split(['-', '+']).next().unwrap_or(rest);
            let mut parts = core.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            return Some((major, minor, patch, token.to_string()));
        }
    }
    None
}

fn raw_first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("(no version output)")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_k6_version_line() {
        let parsed = parse_version("k6 v2.2.0 (commit/abc, go1.24.0, windows/amd64)");
        assert_eq!(parsed, Some((2, 2, 0, "v2.2.0".to_string())));
    }

    #[test]
    fn parses_versions_without_patch() {
        assert_eq!(
            parse_version("k6 v2.3 (commit/abc)"),
            Some((2, 3, 0, "v2.3".to_string()))
        );
    }

    #[test]
    fn rejects_output_without_version() {
        assert_eq!(parse_version("k6 development build"), None);
    }

    #[test]
    fn supported_range_is_two_x() {
        assert!(is_supported_major(2));
        assert!(!is_supported_major(1));
        assert!(!is_supported_major(3));
    }
}
