use super::*;
use crate::platform::Platform;
use crate::resolver::{ResolvedPackageEntry, ResolvedPackages};
use std::collections::HashMap;

#[test]
fn rejects_unmet_locked_platform_requirements() {
    let packages = vec![LockedPackage {
        name: "symfony/console".to_string(),
        version: "8.0.0".to_string(),
        dist_url: "https://example.com/symfony-console.zip".to_string(),
        dist_integrity: Some("blake3:test".to_string()),
        dist_shasum: None,
        package_requires: Vec::new(),
        platform_requires: vec![required_package("php", ">=8.4")],
        dev: false,
    }];
    let platform = platform("8.3.0", &[]);

    let error = validate_locked_platform_requirements(&packages, &platform)
        .unwrap_err()
        .to_string();

    assert!(error.contains("symfony/console"));
    assert!(error.contains("php"));
    assert!(error.contains(">=8.4"));
    assert!(error.contains("8.3.0"));
}

#[test]
fn rejects_unmet_resolved_platform_requirements() {
    let mut packages = HashMap::new();
    packages.insert(
        "symfony/console".to_string(),
        ResolvedPackageEntry {
            version: "8.0.0".to_string(),
            dist_url: "https://example.com/symfony-console.zip".to_string(),
            dist_shasum: None,
            constraints: vec!["^8.0".to_string()],
            package_requires: Vec::new(),
            platform_requires: vec![required_package("ext-intl", "*")],
            provides: Vec::new(),
            replaces: Vec::new(),
        },
    );
    let platform = platform("8.3.0", &["json"]);

    let error = validate_resolved_platform_requirements(&packages, &platform)
        .unwrap_err()
        .to_string();

    assert!(error.contains("symfony/console"));
    assert!(error.contains("ext-intl"));
    assert!(error.contains("*"));
    assert!(error.contains("missing"));
}

#[test]
fn lockfile_includes_root_repositories_in_manifest_hash() {
    let repositories = vec![ComposerRepository {
        url: "https://repo.example.com".to_string(),
    }];
    let lockfile = build_lockfile(
        vec![required_package("psr/log", "^3.0")],
        repositories.clone(),
        &[required_package("psr/log", "^3.0")],
        &ResolvedPackages::new(),
        &PackageIntegrities::new(),
    )
    .unwrap();

    assert_eq!(lockfile.root_repositories, repositories);
    assert!(lockfile::matches_root_manifest(
        &lockfile,
        &[required_package("psr/log", "^3.0")],
        &[ComposerRepository {
            url: "https://repo.example.com".to_string(),
        }]
    ));
    assert!(!lockfile::matches_root_manifest(
        &lockfile,
        &[required_package("psr/log", "^3.0")],
        &[ComposerRepository {
            url: "https://repo.changed.example.com".to_string(),
        }]
    ));
}

#[test]
fn lockfile_marks_packages_outside_production_graph_as_dev() {
    let resolved_packages = ResolvedPackages::from([
        ("psr/log".to_string(), resolved_package(&[])),
        (
            "monolog/monolog".to_string(),
            resolved_package(&[required_package("psr/log", "^3.0")]),
        ),
    ]);

    let lockfile = build_lockfile(
        vec![
            required_package("psr/log", "^3.0"),
            required_package("monolog/monolog", "^3.0"),
        ],
        Vec::new(),
        &[required_package("psr/log", "^3.0")],
        &resolved_packages,
        &integrities(&["psr/log", "monolog/monolog"]),
    )
    .unwrap();

    assert!(!locked_package(&lockfile, "psr/log").dev);
    assert!(locked_package(&lockfile, "monolog/monolog").dev);
}

#[test]
fn lockfile_marks_production_provider_as_non_dev() {
    let mut provider = resolved_package(&[]);
    provider.provides = vec![required_package("psr/log-implementation", "1.0.0")];
    let resolved_packages = ResolvedPackages::from([("acme/log".to_string(), provider)]);

    let lockfile = build_lockfile(
        vec![required_package("psr/log-implementation", "^1.0")],
        Vec::new(),
        &[required_package("psr/log-implementation", "^1.0")],
        &resolved_packages,
        &integrities(&["acme/log"]),
    )
    .unwrap();

    assert!(!locked_package(&lockfile, "acme/log").dev);
}

fn locked_package<'a>(lockfile: &'a Lockfile, name: &str) -> &'a LockedPackage {
    lockfile
        .packages
        .iter()
        .find(|package| package.name == name)
        .unwrap()
}

fn required_package(name: &str, constraint: &str) -> RequiredPackage {
    RequiredPackage {
        name: name.to_string(),
        constraint: constraint.to_string(),
    }
}

fn resolved_package(package_requires: &[RequiredPackage]) -> ResolvedPackageEntry {
    ResolvedPackageEntry {
        version: "1.0.0".to_string(),
        dist_url: "https://example.com/package.zip".to_string(),
        dist_shasum: None,
        constraints: Vec::new(),
        package_requires: package_requires.to_vec(),
        platform_requires: Vec::new(),
        provides: Vec::new(),
        replaces: Vec::new(),
    }
}

fn integrities(packages: &[&str]) -> PackageIntegrities {
    packages
        .iter()
        .map(|package| {
            (
                package.to_string(),
                format!("blake3:{}", blake3::hash(package.as_bytes()).to_hex()),
            )
        })
        .collect()
}

fn platform(php_version: &str, extensions: &[&str]) -> Platform {
    Platform {
        php_version: php_version.to_string(),
        extensions: extensions
            .iter()
            .map(|extension| extension.to_string())
            .collect(),
        extension_versions: HashMap::new(),
    }
}
