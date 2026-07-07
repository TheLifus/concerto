use super::*;
use crate::composer::RequiredPackage;
use crate::lockfile::LOCKFILE_VERSION;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn orders_files_like_composer_dependencies_first() {
    let lockfile = Lockfile {
        lockfile_version: LOCKFILE_VERSION,
        root_manifest_hash: "test".to_string(),
        root_requirements: Vec::new(),
        root_repositories: Vec::new(),
        packages: vec![
            package("vendor/a", &[required_package("vendor/b")]),
            package("vendor/b", &[]),
        ],
    };

    let ordered = package_file_order(&lockfile)
        .iter()
        .map(|package| package.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(ordered, vec!["vendor/b", "vendor/a"]);
}

#[test]
fn reads_php_symbols_for_classmap() {
    let content = r#"
    <?php

    namespace App\Domain;

    final class User {}
    interface UserRepository {}
    trait HasEmail {}
    enum UserStatus {}
    "#;

    let symbols = php_symbols(content);

    assert!(symbols.contains(&"App\\Domain\\User".to_string()));
    assert!(symbols.contains(&"App\\Domain\\UserRepository".to_string()));
    assert!(symbols.contains(&"App\\Domain\\HasEmail".to_string()));
    assert!(symbols.contains(&"App\\Domain\\UserStatus".to_string()));
}

#[test]
fn ignores_anonymous_classes_in_classmap() {
    let content = "<?php $object = new class {}; class Named {}";

    let symbols = php_symbols(content);

    assert_eq!(symbols, vec!["Named"]);
}

#[test]
fn collects_composer_autoload_sections() {
    let project = temp_project("autoload-sections");
    let src = project.join("src");
    let legacy = project.join("legacy");
    let helpers = project.join("helpers.php");

    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(
        src.join("Mapped.php"),
        "<?php namespace App; class Mapped {}",
    )
    .unwrap();
    std::fs::write(&helpers, "<?php function concerto_test_helper() {}").unwrap();

    let composer_json = serde_json::json!({
        "autoload": {
            "psr-4": { "App\\": "src/" },
            "psr-0": { "Legacy_": "legacy/" },
            "files": ["helpers.php"],
            "classmap": ["src/"]
        }
    });
    let mut autoload = AutoloadMap::default();

    collect_autoload_sections(&mut autoload, &composer_json, &project).unwrap();

    assert_eq!(
        autoload.psr4["App\\"],
        vec![absolute_path(project.join("src/")).unwrap()]
    );
    assert_eq!(
        autoload.psr0["Legacy_"],
        vec![absolute_path(project.join("legacy/")).unwrap()]
    );
    assert_eq!(autoload.files, vec![absolute_path(helpers).unwrap()]);
    assert_eq!(
        autoload.classmap["App\\Mapped"],
        absolute_path(src.join("Mapped.php")).unwrap()
    );
}

fn package(name: &str, package_requires: &[RequiredPackage]) -> LockedPackage {
    LockedPackage {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        dist_url: "https://example.com/package.zip".to_string(),
        dist_integrity: Some("blake3:test".to_string()),
        dist_shasum: None,
        dev: false,
        package_requires: package_requires.to_vec(),
        platform_requires: Vec::new(),
    }
}

fn required_package(name: &str) -> RequiredPackage {
    RequiredPackage {
        name: name.to_string(),
        constraint: "*".to_string(),
    }
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
