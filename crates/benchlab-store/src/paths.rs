//! Application data paths and the single-instance lock.

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    data_dir: PathBuf,
}

impl AppPaths {
    /// Resolve the platform data directory, honoring `BENCHLAB_DATA_DIR`.
    pub fn discover() -> Result<Self, PathError> {
        if let Some(override_dir) = env::var_os("BENCHLAB_DATA_DIR") {
            let path = PathBuf::from(override_dir);
            if !path.is_absolute() {
                return Err(PathError::OverrideNotAbsolute(path));
            }
            return Ok(Self { data_dir: path });
        }

        let base = dirs::data_dir().ok_or(PathError::NoDataDirectory)?;
        let name = if cfg!(target_os = "linux") {
            "benchlab"
        } else {
            "BenchLab"
        };
        Ok(Self {
            data_dir: base.join(name),
        })
    }

    /// Build paths rooted at an explicit directory. Tests use this directly.
    pub fn at(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("benchlab.db")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.data_dir.join("benchlab.lock")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn runs_dir(&self) -> PathBuf {
        self.data_dir.join("runs")
    }

    pub fn ensure_directories(&self) -> io::Result<()> {
        fs::create_dir_all(&self.data_dir)?;
        fs::create_dir_all(self.logs_dir())?;
        fs::create_dir_all(self.runs_dir())?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("the operating system did not provide an application data directory")]
    NoDataDirectory,
    #[error("BENCHLAB_DATA_DIR must be an absolute path: {0}")]
    OverrideNotAbsolute(PathBuf),
}

/// An exclusive process lock that lives for the lifetime of the app.
#[derive(Debug)]
pub struct InstanceLock {
    file: File,
    path: PathBuf,
}

impl InstanceLock {
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self, LockError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;

        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => {}
            // Windows reports lock contention as ERROR_LOCK_VIOLATION rather
            // than mapping it to WouldBlock. Unix uses WouldBlock/EAGAIN.
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.raw_os_error() == Some(33) =>
            {
                return Err(LockError::AlreadyRunning(path));
            }
            Err(error) => return Err(LockError::Io(error)),
        }

        // The lock is advisory; the PID is only diagnostic.
        file.set_len(0)?;
        writeln!(file, "pid={}", std::process::id())?;
        file.flush()?;

        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("BenchLab is already running (lock: {0})")]
    AlreadyRunning(PathBuf),
    #[error("could not acquire the single-instance lock: {0}")]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_rooted_at_the_data_directory() {
        let paths = AppPaths::at(std::env::temp_dir().join("benchlab-paths-test"));
        assert!(paths.db_path().ends_with("benchlab.db"));
        assert!(paths.logs_dir().ends_with("logs"));
        assert!(paths.runs_dir().ends_with("runs"));
        assert!(paths.lock_path().ends_with("benchlab.lock"));
    }

    #[test]
    fn second_lock_is_rejected_and_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("benchlab.lock");
        let first = InstanceLock::acquire(&path).unwrap();
        assert!(matches!(
            InstanceLock::acquire(&path),
            Err(LockError::AlreadyRunning(_))
        ));
        drop(first);
        assert!(InstanceLock::acquire(&path).is_ok());
    }
}
