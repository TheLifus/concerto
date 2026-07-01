use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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

fn install(project: &Path) -> Output {
    concerto_command()
        .arg("install")
        .current_dir(project)
        .output()
        .unwrap()
}

fn debug_install(project: &Path) -> Output {
    concerto_command()
        .arg("install")
        .current_dir(project)
        .env("CONCERTO_DEBUG_PERF", "1")
        .output()
        .unwrap()
}

fn timed_debug_install(project: &Path) -> (Output, u128) {
    let started_at = Instant::now();
    let output = debug_install(project);

    (output, started_at.elapsed().as_millis())
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

fn locked_package_count(lockfile: &Value) -> usize {
    lockfile["packages"].as_array().unwrap().len()
}

fn count_log_event(perf_log: &str, event: &str) -> usize {
    perf_log
        .lines()
        .filter(|line| line.starts_with(event))
        .count()
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

#[test]
#[ignore = "hits Packagist and GitHub"]
fn e2e_installs_direct_requirement_and_writes_lockfile() {
    let project = temp_project("direct-requirement");
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"psr/log":"^3.0"}}"#,
    )
    .unwrap();

    let output = install(&project);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(project.join("vendor/psr/log").exists());

    let lockfile = read_lockfile(&project);

    assert!(locked_version(&lockfile, "psr/log").starts_with("3."));
    assert_eq!(lockfile["root_requirements"][0]["name"], "psr/log");
    assert_eq!(lockfile["root_requirements"][0]["constraint"], "^3.0");
}

#[test]
#[ignore = "hits Packagist and GitHub"]
fn e2e_installs_transitive_requirement_from_lockfile() {
    let project = temp_project("transitive-lockfile");
    std::fs::write(
        project.join("composer.json"),
        r#"{"require":{"monolog/monolog":"^3.0"}}"#,
    )
    .unwrap();

    let output = install(&project);

    assert!(output.status.success(), "{}", stderr(&output));

    let lockfile = read_lockfile(&project);

    assert!(locked_version(&lockfile, "monolog/monolog").starts_with("3."));
    assert!(locked_version(&lockfile, "psr/log").starts_with("3."));

    std::fs::remove_dir_all(project.join("vendor")).unwrap();

    let output = concerto_command()
        .arg("install")
        .current_dir(&project)
        .env("CONCERTO_DEBUG_PERF", "1")
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Installing from lockfile"));
    assert!(project.join("vendor/monolog/monolog").exists());

    let perf_log = std::fs::read_to_string(project.join(".concerto/logs/perf.log")).unwrap();

    assert!(perf_log.contains("lockfile_install"));
}

#[test]
#[ignore = "hits Packagist and GitHub"]
fn e2e_prints_install_stats_for_common_cases() {
    let cases = [
        ("direct", r#"{"require":{"psr/log":"^3.0"}}"#),
        ("transitive", r#"{"require":{"monolog/monolog":"^3.0"}}"#),
        (
            "multi",
            r#"{
              "require": {
                "monolog/monolog": "^3.0",
                "symfony/console": "^8.0",
                "guzzlehttp/guzzle": "^7.0",
                "ramsey/uuid": "^4.0",
                "league/flysystem": "^3.0"
              }
            }"#,
        ),
    ];

    println!();
    println!(
        "{:<11} {:>4} {:>6} {:>8} {:>8} {:>7} {:>7} {:>9} {:>7}",
        "case", "root", "locked", "cold_ms", "lock_ms", "speedup", "resolve", "download", "reuse"
    );

    for (name, composer_json) in cases {
        let project = temp_project(name);
        std::fs::write(project.join("composer.json"), composer_json).unwrap();

        let (cold_output, cold_ms) = timed_debug_install(&project);

        assert!(cold_output.status.success(), "{}", stderr(&cold_output));

        let lockfile = read_lockfile(&project);
        let root_count = lockfile["root_requirements"].as_array().unwrap().len();
        let package_count = locked_package_count(&lockfile);

        std::fs::remove_dir_all(project.join("vendor")).unwrap();

        let (lock_output, lock_ms) = timed_debug_install(&project);

        assert!(lock_output.status.success(), "{}", stderr(&lock_output));
        assert!(stdout(&lock_output).contains("Installing from lockfile"));

        let perf_log = std::fs::read_to_string(project.join(".concerto/logs/perf.log")).unwrap();
        let resolve_count = count_log_event(&perf_log, "resolve_package");
        let download_count = count_log_event(&perf_log, "source_download_extract");
        let reuse_count = count_log_event(&perf_log, "source_reuse");
        let speedup = cold_ms.max(1) / lock_ms.max(1);

        println!(
            "{name:<11} {root_count:>4} {package_count:>6} {cold_ms:>8} \
             {lock_ms:>8} {speedup:>6}x {resolve_count:>7} \
             {download_count:>9} {reuse_count:>7}"
        );
    }
}
