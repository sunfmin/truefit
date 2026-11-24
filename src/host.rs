//! What every command reads and does on this host: its records under the repo
//! root, the programs it finds, git, and scratch directories.

use crate::fault;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Writes `value` to `root/file`, or the fault that it cannot.
pub fn save(root: &Path, file: &str, value: &Value) -> Result<(), Vec<Value>> {
    write(root, file, value).map_err(|e| {
        vec![fault(
            format!("cannot write {file}: {e}"),
            "make .truefit/ a directory truefit can write",
        )]
    })
}

/// Writes `value` to `root/file`, making the directories it is in.
pub fn write(root: &Path, file: &str, value: &Value) -> std::io::Result<()> {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap_or(root))?;
    std::fs::write(root.join(file), format!("{value:#}\n"))
}

/// What `git <args>` prints, run in `root`, or None: it failed.
pub fn git(root: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// The JSON record at `root/file`, or null: it is missing or no JSON.
pub fn read(root: &Path, file: &str) -> Value {
    std::fs::read_to_string(root.join(file))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null)
}

/// Where `program` is: relative to `root` when it names a path, else on PATH.
pub fn find(root: &Path, program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let p = root.join(program);
        return p.is_file().then_some(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

/// A fresh directory of its own under the system temp dir; its user removes it.
pub fn scratch() -> Result<PathBuf, String> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "truefit-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    Ok(dir)
}
