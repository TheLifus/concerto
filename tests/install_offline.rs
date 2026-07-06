use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const ARCHIVE_BASE_PLACEHOLDER: &str = "__ARCHIVE_BASE_URL__";
const PACKAGIST_FIXTURES_ENV: &str = "CONCERTO_PACKAGIST_FIXTURES_DIR";
const PLATFORM_EXTENSIONS_ENV: &str = "CONCERTO_PLATFORM_EXTENSIONS";
const PLATFORM_PHP_ENV: &str = "CONCERTO_PLATFORM_PHP";

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

fn fixture_path(relative_path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn read_lockfile(project: &Path) -> Value {
    let content = std::fs::read_to_string(project.join("concerto.lock")).unwrap();

    serde_json::from_str(&content).unwrap()
}

fn locked_version(lockfile: &Value, package_name: &str) -> String {
    lockfile["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == package_name)
        .and_then(|package| package["version"].as_str())
        .unwrap()
        .to_string()
}

fn prepare_packagist_fixtures(project: &Path) -> PathBuf {
    let metadata_dir = project.join("packagist-fixtures");
    let archive_base = format!(
        "file://{}",
        fixture_path("tests/fixtures/archives").display()
    );

    std::fs::create_dir_all(&metadata_dir).unwrap();

    for fixture in ["psr-log", "monolog-monolog", "acme-platform-lock"] {
        let content = std::fs::read_to_string(fixture_path(&format!(
            "tests/fixtures/packagist/{fixture}.json"
        )))
        .unwrap()
        .replace(ARCHIVE_BASE_PLACEHOLDER, &archive_base);

        std::fs::write(metadata_dir.join(format!("{fixture}.json")), content).unwrap();
    }

    metadata_dir
}

fn offline_install(project: &Path, metadata_dir: &Path) -> Output {
    concerto_command()
        .arg("install")
        .current_dir(project)
        .env(PACKAGIST_FIXTURES_ENV, metadata_dir)
        .env(PLATFORM_PHP_ENV, "8.2.25")
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .output()
        .unwrap()
}

#[test]
fn offline_installs_transitive_requirement_and_relinks_from_lockfile() {
    let project = temp_project("offline-transitive");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/autoload.php").exists());
    assert!(project.join("vendor/monolog/monolog").exists());
    assert!(project.join("vendor/psr/log").exists());

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "monolog/monolog"), "3.0.0");
    assert_eq!(locked_version(&lockfile, "psr/log"), "3.0.2");

    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = concerto_command()
        .arg("install")
        .current_dir(&project)
        .env(PACKAGIST_FIXTURES_ENV, project.join("missing-fixtures"))
        .env(PLATFORM_PHP_ENV, "8.2.25")
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Installing from lockfile"));
    assert!(project.join("vendor/monolog/monolog").exists());
    assert!(project.join("vendor/psr/log").exists());
}

#[test]
fn offline_rejects_unmet_platform_requirement() {
    let project = temp_project("offline-platform");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"acme/platform-lock":"^1.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("acme/platform-lock"));
    assert!(error.contains("php"));
    assert!(error.contains(">=999.0"));
    assert!(error.contains("8.2.25"));
    assert!(!project.join("vendor/acme/platform-lock").exists());
}
