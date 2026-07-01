use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const NO_COMPOSER_JSON: &str = "No composer.json found";
const REQUIRE_MUST_BE_OBJECT: &str = "composer.json require must be an object";
const USAGE: &str = "Usage: concerto install";

fn concerto_command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_concerto"))
}

fn temp_project(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("concerto-{name}-{nanos}"));

    std::fs::create_dir_all(&path).unwrap();

    path
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn prints_help_without_command() {
    let output = concerto_command().output().unwrap();

    assert!(output.status.success());
    assert!(stdout(&output).contains(USAGE));
}

#[test]
fn fails_install_without_composer_json() {
    let project = temp_project("missing-composer-json");
    let output = concerto_command()
        .arg("install")
        .current_dir(project)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(stderr(&output).contains(NO_COMPOSER_JSON));
}

#[test]
fn fails_install_when_require_is_missing() {
    let project = temp_project("missing-require");
    std::fs::write(project.join("composer.json"), "{}").unwrap();

    let output = concerto_command()
        .arg("install")
        .current_dir(project)
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(stderr(&output).contains(REQUIRE_MUST_BE_OBJECT));
}
