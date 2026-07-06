use super::*;
use crate::platform::Platform;
use crate::resolver::ResolvedPackageEntry;
use std::collections::HashMap;

#[test]
fn rejects_unmet_locked_platform_requirements() {
    let packages = vec![LockedPackage {
        name: "symfony/console".to_string(),
        version: "8.0.0".to_string(),
        dist_url: "https://example.com/symfony-console.zip".to_string(),
        package_requires: Vec::new(),
        platform_requires: vec![required_package("php", ">=8.4")],
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
            metadata_url: "https://repo.packagist.org/p2/symfony/console.json".to_string(),
            constraints: vec!["^8.0".to_string()],
            package_requires: Vec::new(),
            platform_requires: vec![required_package("ext-intl", "*")],
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

fn required_package(name: &str, constraint: &str) -> RequiredPackage {
    RequiredPackage {
        name: name.to_string(),
        constraint: constraint.to_string(),
    }
}

fn platform(php_version: &str, extensions: &[&str]) -> Platform {
    Platform {
        php_version: php_version.to_string(),
        extensions: extensions
            .iter()
            .map(|extension| extension.to_string())
            .collect(),
    }
}
