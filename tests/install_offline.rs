mod support;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Output;

use support::{concerto_command, locked_version, read_lockfile, stderr, stdout, temp_project};

const ARCHIVE_BASE_PLACEHOLDER: &str = "__ARCHIVE_BASE_URL__";
const PACKAGIST_FIXTURES_ENV: &str = "CONCERTO_PACKAGIST_FIXTURES_DIR";
const PLATFORM_EXTENSIONS_ENV: &str = "CONCERTO_PLATFORM_EXTENSIONS";
const PLATFORM_PHP_ENV: &str = "CONCERTO_PLATFORM_PHP";

fn fixture_path(relative_path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path)
}

fn write_lockfile(project: &Path, lockfile: &Value) {
    let content = serde_json::to_string_pretty(lockfile).unwrap();

    std::fs::write(project.join("concerto.lock"), content).unwrap();
}

fn prepare_packagist_fixtures(project: &Path) -> PathBuf {
    let metadata_dir = project.join("packagist-fixtures");
    let archive_base = format!(
        "file://{}",
        fixture_path("tests/fixtures/archives").display()
    );

    std::fs::create_dir_all(&metadata_dir).unwrap();

    for fixture in [
        "psr-log",
        "monolog-monolog",
        "acme-platform-lock",
        "acme-platform-choice",
    ] {
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
    offline_install_with_platform(project, metadata_dir, "8.2.25")
}

fn offline_install_with_platform(project: &Path, metadata_dir: &Path, php_version: &str) -> Output {
    concerto_command()
        .arg("install")
        .current_dir(project)
        .env(PACKAGIST_FIXTURES_ENV, metadata_dir)
        .env(PLATFORM_PHP_ENV, php_version)
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .output()
        .unwrap()
}

fn offline_lockfile_install(project: &Path, php_version: &str) -> Output {
    concerto_command()
        .arg("install")
        .current_dir(project)
        .env(PACKAGIST_FIXTURES_ENV, project.join("missing-fixtures"))
        .env(PLATFORM_PHP_ENV, php_version)
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .output()
        .unwrap()
}

#[test]
fn offline_installs_direct_requirement() {
    let project = temp_project("offline-direct");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/autoload.php").exists());
    assert!(project.join("vendor/psr/log").exists());
    assert!(!project.join("vendor/monolog/monolog").exists());

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "psr/log"), "3.0.2");
    assert_eq!(lockfile["packages"].as_array().unwrap().len(), 1);
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

    let output = offline_lockfile_install(&project, "8.2.25");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Installing from lockfile"));
    assert!(project.join("vendor/monolog/monolog").exists());
    assert!(project.join("vendor/psr/log").exists());
}

#[test]
fn offline_relinks_from_lockfile_without_archive_url() {
    let project = temp_project("offline-lockfile-no-download");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let mut lockfile = read_lockfile(&project);
    lockfile["packages"][0]["dist_url"] = Value::String("file:///missing/package.zip".to_string());
    write_lockfile(&project, &lockfile);

    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Installing from lockfile"));
    assert!(stdout(&output).contains("Reusing"));
    assert!(project.join("vendor/psr/log").exists());
}

#[test]
fn offline_reports_missing_lockfile_source() {
    let project = temp_project("offline-lockfile-missing-source");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let mut lockfile = read_lockfile(&project);
    lockfile["packages"][0]["dist_url"] = Value::String("file:///missing/package.zip".to_string());
    write_lockfile(&project, &lockfile);

    std::fs::remove_dir_all(project.join("vendor")).unwrap();
    std::fs::remove_dir_all(project.join(".concerto/store/psr/log/3.0.2/source")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("psr/log"));
    assert!(error.contains("file:///missing/package.zip"));
    assert!(!project.join("vendor/psr/log").exists());
}

#[test]
fn offline_validates_platform_before_lockfile_relink() {
    let project = temp_project("offline-lockfile-platform");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"acme/platform-lock":"^1.0"}}"#,
    )
    .unwrap();

    let output = offline_install_with_platform(&project, &metadata_dir, "999.0.0");

    assert!(output.status.success(), "{}", stderr(&output));

    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("acme/platform-lock"));
    assert!(error.contains("php >=999.0 required, detected 8.2.25"));
    assert!(!project.join("vendor/acme/platform-lock").exists());
}

#[test]
fn offline_rejects_unmatched_version_constraint() {
    let project = temp_project("offline-version-conflict");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^2.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("psr/log"));
    assert!(error.contains("^2.0"));
    assert!(!project.join("vendor/psr/log").exists());
    assert!(!project.join("concerto.lock").exists());
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
    assert!(error.contains("^1.0"));
    assert!(error.contains("current platform"));
    assert!(error.contains("php 8.2.25"));
    assert!(!project.join("vendor/acme/platform-lock").exists());
}

#[test]
fn offline_selects_older_platform_compatible_release() {
    let project = temp_project("offline-platform-choice");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"acme/platform-choice":"^1.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/acme/platform-choice").exists());

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "acme/platform-choice"), "1.0.0");
}
