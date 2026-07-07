use super::*;
use std::collections::HashMap;

#[test]
fn accepts_matching_php_requirement() {
    let requirements = vec![required_package("php", ">=8.1")];
    let platform = platform("8.3.0", &[]);

    let result = validate(&requirements, &platform, "monolog/monolog");

    assert!(result.is_ok());
}

#[test]
fn rejects_unmet_php_requirement() {
    let requirements = vec![required_package("php", ">=8.4")];
    let platform = platform("8.3.0", &[]);

    let error = validate(&requirements, &platform, "symfony/console")
        .unwrap_err()
        .to_string();

    assert!(error.contains("symfony/console"));
    assert!(error.contains("php"));
    assert!(error.contains(">=8.4"));
    assert!(error.contains("8.3.0"));
}

#[test]
fn accepts_installed_extension_requirement() {
    let requirements = vec![required_package("ext-json", "*")];
    let platform = platform("8.3.0", &["json"]);

    let result = validate(&requirements, &platform, "symfony/console");

    assert!(result.is_ok());
}

#[test]
fn rejects_missing_extension_requirement() {
    let requirements = vec![required_package("ext-intl", "*")];
    let platform = platform("8.3.0", &["json"]);

    let error = validate(&requirements, &platform, "symfony/console")
        .unwrap_err()
        .to_string();

    assert!(error.contains("symfony/console"));
    assert!(error.contains("ext-intl"));
    assert!(error.contains("*"));
    assert!(error.contains("missing"));
}

#[test]
fn rejects_library_requirement_as_unsupported() {
    let requirements = vec![required_package("lib-icu", ">=70")];
    let platform = platform("8.3.0", &[]);

    let error = validate(&requirements, &platform, "symfony/intl")
        .unwrap_err()
        .to_string();

    assert!(error.contains("symfony/intl"));
    assert!(error.contains("lib-icu"));
    assert!(error.contains(">=70"));
    assert!(error.contains("unsupported"));
}

#[test]
fn parses_platform_from_php_output() {
    let platform = parse_platform(
        r#"
8.3.1
Core=8.3.1
json=8.3.1
PDO
"#
        .trim_start(),
    )
    .unwrap();

    assert_eq!(platform.php_version, "8.3.1");
    assert_eq!(
        platform.extensions,
        vec!["core".to_string(), "json".to_string(), "pdo".to_string()]
    );
    assert_eq!(platform.extension_versions["json"], "8.3.1");
}

#[test]
fn accepts_matching_extension_version_requirement() {
    let requirements = vec![required_package("ext-json", ">=1.7")];
    let mut platform = platform("8.3.0", &["json"]);
    platform
        .extension_versions
        .insert("json".to_string(), "1.7.0".to_string());

    let result = validate(&requirements, &platform, "symfony/console");

    assert!(result.is_ok());
}

#[test]
fn rejects_unmet_extension_version_requirement() {
    let requirements = vec![required_package("ext-json", ">=2.0")];
    let mut platform = platform("8.3.0", &["json"]);
    platform
        .extension_versions
        .insert("json".to_string(), "1.7.0".to_string());

    let error = validate(&requirements, &platform, "symfony/console")
        .unwrap_err()
        .to_string();

    assert!(error.contains("ext-json"));
    assert!(error.contains(">=2.0"));
    assert!(error.contains("1.7.0"));
}

#[test]
fn rejects_extension_version_requirement_when_version_is_unknown() {
    let requirements = vec![required_package("ext-json", ">=1.0")];
    let platform = platform("8.3.0", &["json"]);

    let error = validate(&requirements, &platform, "symfony/console")
        .unwrap_err()
        .to_string();

    assert!(error.contains("ext-json"));
    assert!(error.contains("version unknown"));
}

#[test]
fn accepts_extension_requirement_case_insensitively() {
    let requirements = vec![required_package("Ext-JSON", "*")];
    let platform = platform("8.3.0", &["json"]);

    let result = validate(&requirements, &platform, "symfony/console");

    assert!(result.is_ok());
}

#[test]
fn accepts_platform_requirement_name_case_insensitively() {
    let requirements = vec![required_package("PHP", ">=8.1")];
    let platform = platform("8.3.0", &[]);

    let result = validate(&requirements, &platform, "symfony/console");

    assert!(result.is_ok());
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
        extension_versions: HashMap::new(),
    }
}
