//! Helpers shared by the integration tests.
//!
//! No dev-dependencies (the dependency list is deliberately short): temporary
//! directories in `std::env::temp_dir()` with a unique name (pid + counter),
//! cleaned up on `Drop`.

#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A test's own temporary directory.
pub struct Temp {
    root: PathBuf,
}

impl Temp {
    pub fn new(label: &str) -> Temp {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("quotop-test-{}-{label}-{n}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create temporary directory");
        Temp { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes `name` (which may include subdirectories) with `contents` and a
    /// specific mode.
    pub fn file(&self, name: &str, contents: &str, mode: u32) -> PathBuf {
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create the file's directory");
        }
        std::fs::write(&path, contents).expect("write test file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
        path
    }

    /// A test `Source` from the given environment and files.
    pub fn source(&self, env: &[(&str, &str)], files: &[PathBuf]) -> quotop::credentials::Source {
        quotop::credentials::Source {
            env: env
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
            key_files: files.to_vec(),
            home_dir: Some(self.root.clone()),
        }
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A file's raw contents, to compare before/after.
pub fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("read test file")
}
