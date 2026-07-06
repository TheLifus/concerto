use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn concerto_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_concerto"))
}

pub(crate) fn temp_project(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("concerto-{name}-{nanos}"));

    std::fs::create_dir_all(&path).unwrap();

    path
}

pub(crate) fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

pub(crate) fn read_lockfile(project: &Path) -> Value {
    let content = std::fs::read_to_string(project.join("concerto.lock")).unwrap();

    serde_json::from_str(&content).unwrap()
}

pub(crate) fn locked_version(lockfile: &Value, package_name: &str) -> String {
    lockfile["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == package_name)
        .and_then(|package| package["version"].as_str())
        .unwrap()
        .to_string()
}
