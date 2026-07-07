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
        "acme-app",
        "acme-log",
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

fn write_repository_package(repository_dir: &Path, package_name: &str, metadata: &str) {
    let path = repository_dir
        .join("p2")
        .join(package_name)
        .with_extension("json");

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, metadata).unwrap();
}

fn write_repository_provider(repository_dir: &Path, package_name: &str, metadata: &str) {
    let path = repository_dir
        .join("providers")
        .join(package_name)
        .with_extension("json");

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, metadata).unwrap();
}

fn offline_install(project: &Path, metadata_dir: &Path) -> Output {
    offline_install_with_platform(project, metadata_dir, "8.2.25")
}

fn offline_install_no_dev(project: &Path, metadata_dir: &Path) -> Output {
    concerto_command()
        .args(["install", "--no-dev"])
        .current_dir(project)
        .env(PACKAGIST_FIXTURES_ENV, metadata_dir)
        .env(PLATFORM_PHP_ENV, "8.2.25")
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .output()
        .unwrap()
}

fn assert_install_summary(output: &Output, package_count: usize) {
    let package_label = if package_count == 1 {
        "1 package".to_string()
    } else {
        format!("{package_count} packages")
    };

    assert!(
        stdout(output).contains(&format!("Install complete: {package_label} in ")),
        "{}",
        stdout(output)
    );
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

fn offline_debug_install(project: &Path, metadata_dir: &Path) -> Output {
    concerto_command()
        .arg("install")
        .current_dir(project)
        .env(PACKAGIST_FIXTURES_ENV, metadata_dir)
        .env(PLATFORM_PHP_ENV, "8.2.25")
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .env("CONCERTO_DEBUG_PERF", "1")
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

fn offline_lockfile_install_unsafe_trust_store(project: &Path, php_version: &str) -> Output {
    concerto_command()
        .args(["install", "--unsafe-trust-store"])
        .current_dir(project)
        .env(PACKAGIST_FIXTURES_ENV, project.join("missing-fixtures"))
        .env(PLATFORM_PHP_ENV, php_version)
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .output()
        .unwrap()
}

fn offline_debug_lockfile_install(project: &Path, unsafe_trust_store: bool) -> Output {
    let mut command = concerto_command();
    command.arg("install");
    if unsafe_trust_store {
        command.arg("--unsafe-trust-store");
    }

    command
        .current_dir(project)
        .env(PACKAGIST_FIXTURES_ENV, project.join("missing-fixtures"))
        .env(PLATFORM_PHP_ENV, "8.2.25")
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .env("CONCERTO_DEBUG_PERF", "1")
        .output()
        .unwrap()
}

fn offline_lockfile_install_no_dev(project: &Path, php_version: &str) -> Output {
    concerto_command()
        .args(["install", "--no-dev"])
        .current_dir(project)
        .env(PACKAGIST_FIXTURES_ENV, project.join("missing-fixtures"))
        .env(PLATFORM_PHP_ENV, php_version)
        .env(PLATFORM_EXTENSIONS_ENV, "json")
        .output()
        .unwrap()
}

fn locked_dev(lockfile: &Value, package_name: &str) -> bool {
    lockfile["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == package_name)
        .and_then(|package| package["dev"].as_bool())
        .unwrap()
}

fn locked_integrity(lockfile: &Value, package_name: &str) -> String {
    lockfile["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == package_name)
        .and_then(|package| package["dist_integrity"].as_str())
        .unwrap()
        .to_string()
}

fn locked_shasum(lockfile: &Value, package_name: &str) -> Option<String> {
    lockfile["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|package| package["name"] == package_name)
        .and_then(|package| package["dist_shasum"].as_str())
        .map(str::to_string)
}

fn integrity_store_key(integrity: &str) -> String {
    integrity.replace(':', "-")
}

fn fixture_archive_integrity(path: &str) -> String {
    let content = std::fs::read(fixture_path(path)).unwrap();

    format!("blake3:{}", blake3::hash(&content).to_hex())
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
    assert_install_summary(&output, 1);

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "psr/log"), "3.0.2");
    assert_eq!(lockfile["packages"].as_array().unwrap().len(), 1);
    assert!(!locked_dev(&lockfile, "psr/log"));
    assert!(locked_integrity(&lockfile, "psr/log").starts_with("blake3:"));
    assert_eq!(
        locked_shasum(&lockfile, "psr/log"),
        Some("d1b237d28598c3eecb03447d38b3bc30b4baac44".to_string())
    );
}

#[test]
fn offline_rejects_packagist_shasum_mismatch_before_extracting() {
    let project = temp_project("offline-shasum-mismatch");
    let repository_dir = project.join("repo");
    let archive_base = format!(
        "file://{}",
        fixture_path("tests/fixtures/archives").display()
    );
    write_repository_package(
        &repository_dir,
        "psr/log",
        &format!(
            r#"{{
  "packages": {{
    "psr/log": [
      {{
        "version": "3.0.2",
        "dist": {{
          "url": "{archive_base}/psr-log-3.0.2.zip",
          "shasum": "0000000000000000000000000000000000000000"
        }}
      }}
    ]
  }}
}}"#
        ),
    );
    std::fs::write(
        project.join("composer.json"),
        format!(
            r#"{{
  "repositories": [{{"type":"composer","url":"file://{}"}}],
  "require": {{"psr/log":"^3.0"}}
}}"#,
            repository_dir.display()
        ),
    )
    .unwrap();

    let output = offline_install(&project, &project.join("missing-fixtures"));
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("psr/log"));
    assert!(error.contains("archive shasum mismatch"));
    assert!(!project.join("concerto.lock").exists());
    assert!(!project.join("vendor/psr/log").exists());
    assert!(
        !project
            .join(".concerto/store/psr/log/3.0.2/package.zip.tmp")
            .exists()
    );
}

#[test]
fn offline_installs_require_dev_by_default() {
    let project = temp_project("offline-require-dev");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"},"require-dev":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/psr/log").exists());
    assert!(project.join("vendor/monolog/monolog").exists());
    assert_install_summary(&output, 2);

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "psr/log"), "3.0.2");
    assert_eq!(locked_version(&lockfile, "monolog/monolog"), "3.0.0");
    assert!(!locked_dev(&lockfile, "psr/log"));
    assert!(locked_dev(&lockfile, "monolog/monolog"));
    assert!(locked_integrity(&lockfile, "psr/log").starts_with("blake3:"));
    assert!(locked_integrity(&lockfile, "monolog/monolog").starts_with("blake3:"));
    assert!(
        lockfile["root_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|requirement| requirement["name"] == "monolog/monolog")
    );
}

#[test]
fn offline_skips_require_dev_with_no_dev() {
    let project = temp_project("offline-no-dev");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"},"require-dev":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install_no_dev(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/psr/log").exists());
    assert!(!project.join("vendor/monolog/monolog").exists());
    assert_install_summary(&output, 1);

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "psr/log"), "3.0.2");
    assert_eq!(locked_version(&lockfile, "monolog/monolog"), "3.0.0");
    assert_eq!(lockfile["packages"].as_array().unwrap().len(), 2);
    assert!(!locked_dev(&lockfile, "psr/log"));
    assert!(locked_dev(&lockfile, "monolog/monolog"));
    assert!(locked_integrity(&lockfile, "psr/log").starts_with("blake3:"));
    assert!(locked_integrity(&lockfile, "monolog/monolog").starts_with("blake3:"));
    assert!(
        lockfile["root_requirements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|requirement| requirement["name"] == "monolog/monolog")
    );
}

#[test]
fn offline_no_dev_reuses_full_lockfile_without_rewriting() {
    let project = temp_project("offline-no-dev-lockfile");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"},"require-dev":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/monolog/monolog").exists());

    let lockfile_before = std::fs::read_to_string(project.join("concerto.lock")).unwrap();
    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_lockfile_install_no_dev(&project, "8.2.25");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Installing from lockfile"));
    assert_install_summary(&output, 1);
    assert!(project.join("vendor/psr/log").exists());
    assert!(!project.join("vendor/monolog/monolog").exists());

    let lockfile_after = std::fs::read_to_string(project.join("concerto.lock")).unwrap();

    assert_eq!(lockfile_after, lockfile_before);
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
    assert_install_summary(&output, 2);
    assert!(stdout(&output).contains("monolog/monolog 3.0.0 ->"));
    assert!(!stdout(&output).contains("monolog/monolog ^3.0 ->"));

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "monolog/monolog"), "3.0.0");
    assert_eq!(locked_version(&lockfile, "psr/log"), "3.0.2");

    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Installing from lockfile"));
    assert_install_summary(&output, 2);
    assert!(project.join("vendor/monolog/monolog").exists());
    assert!(project.join("vendor/psr/log").exists());
}

#[test]
fn offline_resolved_install_rolls_back_created_vendor_links_on_later_failure() {
    let project = temp_project("offline-resolved-vendor-rollback-created");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(project.join("vendor/psr/log")).unwrap();

    let output = offline_install(&project, &metadata_dir);
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("psr/log"));
    assert!(error.contains("vendor path already exists and is not a symlink"));
    assert!(!project.join("vendor/monolog/monolog").exists());
    assert!(!project.join("vendor/monolog").exists());
    assert!(project.join("vendor/psr/log").is_dir());
    assert!(!project.join("concerto.lock").exists());
}

#[test]
fn offline_lockfile_install_restores_replaced_vendor_links_on_later_failure() {
    let project = temp_project("offline-lockfile-vendor-rollback-replaced");
    let metadata_dir = prepare_packagist_fixtures(&project);
    let old_target = project.join("old-monolog-source");
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    std::fs::create_dir_all(&old_target).unwrap();
    std::fs::remove_file(project.join("vendor/monolog/monolog")).unwrap();
    std::os::unix::fs::symlink(&old_target, project.join("vendor/monolog/monolog")).unwrap();
    std::fs::remove_file(project.join("vendor/psr/log")).unwrap();
    std::fs::create_dir_all(project.join("vendor/psr/log")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("psr/log"));
    assert!(error.contains("vendor path already exists and is not a symlink"));
    assert_eq!(
        std::fs::read_link(project.join("vendor/monolog/monolog")).unwrap(),
        old_target
    );
    assert!(project.join("vendor/psr/log").is_dir());
}

#[test]
fn offline_resolved_install_rolls_back_vendor_links_when_autoload_write_fails() {
    let project = temp_project("offline-resolved-vendor-rollback-autoload");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(project.join("vendor/autoload.php.tmp")).unwrap();

    let output = offline_install(&project, &metadata_dir);
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("Could not write temporary autoload file"));
    assert!(!project.join("vendor/monolog/monolog").exists());
    assert!(!project.join("vendor/monolog").exists());
    assert!(!project.join("vendor/psr/log").exists());
    assert!(!project.join("vendor/psr").exists());
    assert!(!project.join("concerto.lock").exists());
}

#[test]
fn offline_lockfile_install_preserves_existing_autoload_when_rewrite_fails() {
    let project = temp_project("offline-lockfile-autoload-rollback");
    let metadata_dir = prepare_packagist_fixtures(&project);
    let old_autoload = "<?php // old autoload\n";
    let old_loader = "<?php // old loader\n";
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    std::fs::write(project.join("vendor/autoload.php"), old_autoload).unwrap();
    std::fs::write(project.join("vendor/concerto_autoload.php"), old_loader).unwrap();
    std::fs::create_dir_all(project.join("vendor/autoload.php.tmp")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("Could not write temporary autoload file"));
    assert_eq!(
        std::fs::read_to_string(project.join("vendor/autoload.php")).unwrap(),
        old_autoload
    );
    assert_eq!(
        std::fs::read_to_string(project.join("vendor/concerto_autoload.php")).unwrap(),
        old_loader
    );
    assert!(project.join("vendor/psr/log").exists());
}

#[test]
fn offline_resolved_install_rolls_back_when_lockfile_write_fails() {
    let project = temp_project("offline-resolved-lockfile-rollback");
    let metadata_dir = prepare_packagist_fixtures(&project);
    let old_autoload = "<?php // old autoload\n";
    let old_loader = "<?php // old loader\n";
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let old_lockfile = std::fs::read_to_string(project.join("concerto.lock")).unwrap();
    std::fs::write(project.join("vendor/autoload.php"), old_autoload).unwrap();
    std::fs::write(project.join("vendor/concerto_autoload.php"), old_loader).unwrap();
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(project.join("concerto.lock.tmp")).unwrap();

    let output = offline_install(&project, &metadata_dir);
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("Could not write temporary lockfile"));
    assert_eq!(
        std::fs::read_to_string(project.join("concerto.lock")).unwrap(),
        old_lockfile
    );
    assert_eq!(
        std::fs::read_to_string(project.join("vendor/autoload.php")).unwrap(),
        old_autoload
    );
    assert_eq!(
        std::fs::read_to_string(project.join("vendor/concerto_autoload.php")).unwrap(),
        old_loader
    );
    assert!(!project.join("vendor/monolog/monolog").exists());
    assert!(!project.join("vendor/monolog").exists());
    assert!(project.join("vendor/psr/log").exists());
}

#[test]
fn offline_verifies_archive_integrity_when_rebuilding_store_from_lockfile() {
    let project = temp_project("offline-lockfile-verified-download");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    let lockfile = read_lockfile(&project);
    let store_key = integrity_store_key(&locked_integrity(&lockfile, "psr/log"));

    std::fs::remove_dir_all(project.join("vendor")).unwrap();
    std::fs::remove_dir_all(project.join(".concerto/store")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/psr/log").exists());
    assert!(
        project
            .join(format!(
                ".concerto/store/psr/log/3.0.2/{store_key}/integrity"
            ))
            .exists()
    );
}

#[test]
fn offline_rejects_archive_integrity_mismatch_before_extracting() {
    let project = temp_project("offline-lockfile-integrity-mismatch");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let mismatch_archive = format!(
        "file://{}",
        fixture_path("tests/fixtures/archives/monolog-monolog-3.0.0.zip").display()
    );
    let mut lockfile = read_lockfile(&project);
    let store_key = integrity_store_key(&locked_integrity(&lockfile, "psr/log"));
    lockfile["packages"][0]["dist_url"] = Value::String(mismatch_archive);
    write_lockfile(&project, &lockfile);

    std::fs::remove_dir_all(project.join("vendor")).unwrap();
    std::fs::remove_dir_all(project.join(format!(".concerto/store/psr/log/3.0.2/{store_key}")))
        .unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("psr/log"));
    assert!(error.contains("archive integrity mismatch"));
    assert!(error.contains("expected blake3:"));
    assert!(error.contains("observed blake3:"));
    assert!(
        !project
            .join(format!(".concerto/store/psr/log/3.0.2/{store_key}/source"))
            .exists()
    );
    assert!(
        !project
            .join(format!(
                ".concerto/store/psr/log/3.0.2/{store_key}/source.tmp"
            ))
            .exists()
    );
    assert!(
        !project
            .join(".concerto/store/psr/log/3.0.2/package.zip.tmp")
            .exists()
    );
    assert!(!project.join("vendor/psr/log").exists());
}

#[test]
fn offline_does_not_publish_source_when_integrity_marker_write_fails() {
    let project = temp_project("offline-integrity-marker-write-fails");
    let metadata_dir = prepare_packagist_fixtures(&project);
    let integrity = fixture_archive_integrity("tests/fixtures/archives/psr-log-3.0.2.zip");
    let store_key = integrity_store_key(&integrity);
    let store_path = project.join(format!(".concerto/store/psr/log/3.0.2/{store_key}"));

    std::fs::create_dir_all(store_path.join("integrity")).unwrap();
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("could not write archive integrity marker"));
    assert!(!store_path.join("source").exists());
    assert!(!project.join("vendor/psr/log").exists());
}

#[test]
fn offline_rejects_lockfile_package_without_archive_integrity() {
    let project = temp_project("offline-lockfile-missing-integrity");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let mut lockfile = read_lockfile(&project);
    lockfile["packages"][0]
        .as_object_mut()
        .unwrap()
        .remove("dist_integrity");
    write_lockfile(&project, &lockfile);

    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("Missing archive integrity for psr/log"));
    assert!(!project.join("vendor/psr/log").exists());
}

#[test]
fn offline_rejects_tampered_persisted_archive_on_reuse() {
    let project = temp_project("offline-store-zip-tamper");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let lockfile = read_lockfile(&project);
    let store_key = integrity_store_key(&locked_integrity(&lockfile, "psr/log"));
    std::fs::copy(
        fixture_path("tests/fixtures/archives/monolog-monolog-3.0.0.zip"),
        project.join(format!(
            ".concerto/store/psr/log/3.0.2/{store_key}/package.zip"
        )),
    )
    .unwrap();
    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("psr/log"));
    assert!(error.contains("archive integrity mismatch"));
    assert!(!project.join("vendor/psr/log").exists());
}

#[test]
fn offline_unsafe_trust_store_relinks_without_rehashing_archive() {
    let project = temp_project("offline-store-trust");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let lockfile = read_lockfile(&project);
    let store_key = integrity_store_key(&locked_integrity(&lockfile, "psr/log"));
    std::fs::copy(
        fixture_path("tests/fixtures/archives/monolog-monolog-3.0.0.zip"),
        project.join(format!(
            ".concerto/store/psr/log/3.0.2/{store_key}/package.zip"
        )),
    )
    .unwrap();
    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_lockfile_install_unsafe_trust_store(&project, "8.2.25");

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/psr/log").exists());
}

#[test]
fn offline_logs_archive_integrity_phases() {
    let project = temp_project("offline-integrity-perf");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_debug_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_debug_lockfile_install(&project, false);

    assert!(output.status.success(), "{}", stderr(&output));
    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_debug_lockfile_install(&project, true);

    assert!(output.status.success(), "{}", stderr(&output));

    let perf_log = std::fs::read_to_string(project.join(".concerto/logs/perf.log")).unwrap();

    assert!(perf_log.contains("archive_hash_download"));
    assert!(perf_log.contains("archive_hash_reuse"));
    assert!(perf_log.contains("archive_trust_reuse"));
    assert!(perf_log.contains("sha1=true"));
    assert!(perf_log.contains("sha1=false"));
    assert!(perf_log.contains("platform_current"));
    assert!(perf_log.contains("mode=php"));
}

#[test]
fn offline_lockfile_with_extension_requirement_uses_full_platform_detection() {
    let project = temp_project("offline-lockfile-platform-full");
    let repository_dir = project.join("repo");
    let archive_base = format!(
        "file://{}",
        fixture_path("tests/fixtures/archives").display()
    );
    write_repository_package(
        &repository_dir,
        "acme/ext-lock",
        &format!(
            r#"{{
  "packages": {{
    "acme/ext-lock": [
      {{
        "version": "1.0.0",
        "dist": {{"url": "{archive_base}/psr-log-3.0.2.zip"}},
        "require": {{"ext-json": "*"}}
      }}
    ]
  }}
}}"#
        ),
    );
    std::fs::write(
        project.join("composer.json"),
        format!(
            r#"{{
  "repositories": [{{"type":"composer","url":"file://{}"}}],
  "require": {{"acme/ext-lock":"^1.0"}}
}}"#,
            repository_dir.display()
        ),
    )
    .unwrap();

    let output = offline_install(&project, &project.join("missing-fixtures"));

    assert!(output.status.success(), "{}", stderr(&output));
    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_debug_lockfile_install(&project, false);

    assert!(output.status.success(), "{}", stderr(&output));

    let perf_log = std::fs::read_to_string(project.join(".concerto/logs/perf.log")).unwrap();

    assert!(perf_log.contains("platform_current"));
    assert!(perf_log.contains("mode=full"));
}

#[test]
fn offline_stores_same_package_version_by_archive_integrity() {
    let project = temp_project("offline-content-addressed-store");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let mut lockfile = read_lockfile(&project);
    let original_key = integrity_store_key(&locked_integrity(&lockfile, "psr/log"));
    let alternate_integrity =
        fixture_archive_integrity("tests/fixtures/archives/monolog-monolog-3.0.0.zip");
    let alternate_key = integrity_store_key(&alternate_integrity);
    lockfile["packages"][0]["dist_url"] = Value::String(format!(
        "file://{}",
        fixture_path("tests/fixtures/archives/monolog-monolog-3.0.0.zip").display()
    ));
    lockfile["packages"][0]["dist_integrity"] = Value::String(alternate_integrity);
    lockfile["packages"][0]["dist_shasum"] =
        Value::String("ee258ba725ddd60cd74921ebd8e3e21ff021bf20".to_string());
    write_lockfile(&project, &lockfile);

    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = offline_lockfile_install(&project, "8.2.25");

    assert!(output.status.success(), "{}", stderr(&output));
    assert_ne!(original_key, alternate_key);
    assert!(
        project
            .join(format!(
                ".concerto/store/psr/log/3.0.2/{original_key}/source"
            ))
            .exists()
    );
    assert!(
        project
            .join(format!(
                ".concerto/store/psr/log/3.0.2/{alternate_key}/source"
            ))
            .exists()
    );
}

#[test]
fn offline_prints_final_summary_after_cold_store_rebuild() {
    let project = temp_project("offline-cold-summary");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_install_summary(&output, 2);
    assert!(stdout(&output).contains("Generated autoload for 2 packages"));

    std::fs::remove_dir_all(project.join("vendor")).unwrap();
    std::fs::remove_dir_all(project.join(".concerto")).unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert_install_summary(&output, 2);
    assert!(stdout(&output).contains("Generated autoload for 2 packages"));
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
fn offline_rejects_existing_vendor_directory() {
    let project = temp_project("offline-existing-vendor-directory");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(project.join("vendor/psr/log")).unwrap();

    let output = offline_install(&project, &metadata_dir);
    let error = stderr(&output);

    assert!(!output.status.success());
    assert!(error.contains("Could not link psr/log"));
    assert!(error.contains("vendor path already exists and is not a symlink"));
    assert!(error.contains("Remove or move the existing vendor path"));
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
    let store_key = integrity_store_key(&locked_integrity(&lockfile, "psr/log"));
    lockfile["packages"][0]["dist_url"] = Value::String("file:///missing/package.zip".to_string());
    write_lockfile(&project, &lockfile);

    std::fs::remove_dir_all(project.join("vendor")).unwrap();
    std::fs::remove_dir_all(
        project.join(format!(".concerto/store/psr/log/3.0.2/{store_key}/source")),
    )
    .unwrap();

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
fn offline_installs_provider_for_virtual_requirement() {
    let project = temp_project("offline-virtual-provider");
    let metadata_dir = prepare_packagist_fixtures(&project);
    let archive_base = format!(
        "file://{}",
        fixture_path("tests/fixtures/archives").display()
    );
    std::fs::write(
        metadata_dir.join("providers-psr-log-implementation.json"),
        r#"{"providers":[{"name":"acme/bad-log"},{"name":"acme/log"}]}"#,
    )
    .unwrap();
    std::fs::write(
        metadata_dir.join("acme-bad-log.json"),
        format!(
            r#"{{
  "packages": {{
    "acme/bad-log": [
      {{
        "version": "1.0.0",
        "dist": {{ "url": "{archive_base}/psr-log-3.0.2.zip" }},
        "require": {{ "ext-ldap": "*" }},
        "provide": {{ "psr/log-implementation": "1.0.0" }}
      }}
    ]
  }}
}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        metadata_dir.join("acme-log.json"),
        format!(
            r#"{{
  "packages": {{
    "acme/log": [
      {{
        "version": "1.0.0",
        "dist": {{ "url": "{archive_base}/psr-log-3.0.2.zip" }},
        "provide": {{ "psr/log-implementation": "1.0.0" }}
      }}
    ]
  }}
}}"#
        ),
    )
    .unwrap();
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log-implementation":"^1.0"}}"#,
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/acme/log").exists());
    assert_install_summary(&output, 1);

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "acme/log"), "1.0.0");
}

#[test]
fn offline_uses_custom_repository_before_packagist_fallback() {
    let project = temp_project("offline-custom-repo-priority");
    let metadata_dir = prepare_packagist_fixtures(&project);
    let repository_dir = project.join("custom-repo");
    let archive_base = format!(
        "file://{}",
        fixture_path("tests/fixtures/archives").display()
    );
    write_repository_package(
        &repository_dir,
        "psr/log",
        &format!(
            r#"{{
  "packages": {{
    "psr/log": [
      {{
        "version": "3.1.0",
        "dist": {{ "url": "{archive_base}/psr-log-3.0.2.zip" }}
      }}
    ]
  }}
}}"#
        ),
    );
    std::fs::write(
        project.join("composer.json"),
        format!(
            r#"{{
  "repositories": [
    {{ "type": "composer", "url": "file://{}" }}
  ],
  "require": {{ "psr/log": "^3.0" }}
}}"#,
            repository_dir.display()
        ),
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "psr/log"), "3.1.0");
}

#[test]
fn offline_falls_back_to_packagist_fixtures_after_custom_repository_miss() {
    let project = temp_project("offline-custom-repo-fallback");
    let metadata_dir = prepare_packagist_fixtures(&project);
    let repository_dir = project.join("empty-repo");
    std::fs::create_dir_all(&repository_dir).unwrap();
    std::fs::write(
        project.join("composer.json"),
        format!(
            r#"{{
  "repositories": [
    {{ "type": "composer", "url": "file://{}" }}
  ],
  "require": {{ "psr/log": "^3.0" }}
}}"#,
            repository_dir.display()
        ),
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "psr/log"), "3.0.2");
}

#[test]
fn offline_uses_custom_repository_provider_metadata() {
    let project = temp_project("offline-custom-repo-provider");
    let metadata_dir = prepare_packagist_fixtures(&project);
    let repository_dir = project.join("provider-repo");
    let archive_base = format!(
        "file://{}",
        fixture_path("tests/fixtures/archives").display()
    );
    write_repository_provider(
        &repository_dir,
        "psr/log-implementation",
        r#"{"providers":[{"name":"acme/log"}]}"#,
    );
    write_repository_package(
        &repository_dir,
        "acme/log",
        &format!(
            r#"{{
  "packages": {{
    "acme/log": [
      {{
        "version": "1.0.0",
        "dist": {{ "url": "{archive_base}/psr-log-3.0.2.zip" }},
        "provide": {{ "psr/log-implementation": "1.0.0" }}
      }}
    ]
  }}
}}"#
        ),
    );
    std::fs::write(
        project.join("composer.json"),
        format!(
            r#"{{
  "repositories": [
    {{ "type": "composer", "url": "file://{}" }}
  ],
  "require": {{ "psr/log-implementation": "^1.0" }}
}}"#,
            repository_dir.display()
        ),
    )
    .unwrap();

    let output = offline_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "acme/log"), "1.0.0");
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

#[test]
fn offline_backtracks_when_latest_candidate_conflicts_later() {
    let project = temp_project("offline-backtracking");
    let metadata_dir = prepare_packagist_fixtures(&project);
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"acme/app":"^1.0","acme/log":"^2.0"}}"#,
    )
    .unwrap();

    let output = offline_debug_install(&project, &metadata_dir);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/acme/app").exists());
    assert!(project.join("vendor/acme/log").exists());
    assert_install_summary(&output, 2);

    let lockfile = read_lockfile(&project);

    assert_eq!(locked_version(&lockfile, "acme/app"), "1.0.0");
    assert_eq!(locked_version(&lockfile, "acme/log"), "2.0.0");

    let perf_log = std::fs::read_to_string(project.join(".concerto/logs/perf.log")).unwrap();

    assert!(perf_log.contains("resolve_candidates"));
    assert!(perf_log.contains("resolve_solver"));
    assert!(perf_log.contains("versions="));
    assert!(perf_log.contains("dependencies="));
    assert!(perf_log.contains("provider_versions="));
}
