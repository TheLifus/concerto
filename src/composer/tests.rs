use super::*;

#[test]
fn reads_required_packages() {
    let composer_json = r#"
    {
        "require": {
            "monolog/monolog": "^3.0"
        }
    }
    "#;

    let packages = required_packages(composer_json).unwrap();

    assert_eq!(
        packages,
        vec![RequiredPackage {
            name: "monolog/monolog".to_string(),
            constraint: "^3.0".to_string(),
        }]
    );
}

#[test]
fn reads_manifest_require_dev_and_platform_requirements_separately() {
    let composer_json = r#"
    {
        "require": {
            "php": "^8.2",
            "ext-json": "*",
            "monolog/monolog": "^3.0"
        },
        "require-dev": {
            "phpunit/phpunit": "^11.0"
        }
    }
    "#;

    let manifest = manifest(composer_json).unwrap();

    assert_eq!(
        manifest.package_requirements,
        vec![RequiredPackage {
            name: "monolog/monolog".to_string(),
            constraint: "^3.0".to_string(),
        }]
    );
    assert_eq!(
        manifest.platform_requirements,
        vec![
            RequiredPackage {
                name: "ext-json".to_string(),
                constraint: "*".to_string(),
            },
            RequiredPackage {
                name: "php".to_string(),
                constraint: "^8.2".to_string(),
            },
        ]
    );
    assert_eq!(
        manifest.require_dev,
        vec![RequiredPackage {
            name: "phpunit/phpunit".to_string(),
            constraint: "^11.0".to_string(),
        }]
    );
}

#[test]
fn rejects_scripts_with_clear_error() {
    let error = manifest(r#"{"require":{},"scripts":{"post-install-cmd":"echo nope"}}"#)
        .unwrap_err()
        .to_string();

    assert!(error.contains("scripts are not supported yet"));
}

#[test]
fn rejects_plugins_with_clear_error() {
    let error = manifest(r#"{"require":{},"config":{"allow-plugins":{"acme/plugin":true}}}"#)
        .unwrap_err()
        .to_string();

    assert!(error.contains("plugins are not supported yet"));
}

#[test]
fn accepts_suggest_as_documented_noop() {
    let manifest = manifest(
        r#"
        {
            "require": {
                "psr/log": "^3.0"
            },
            "suggest": {
                "ext-intl": "Improves Unicode handling"
            }
        }
        "#,
    )
    .unwrap();

    assert_eq!(
        manifest.package_requirements,
        vec![RequiredPackage {
            name: "psr/log".to_string(),
            constraint: "^3.0".to_string(),
        }]
    );
}

#[test]
fn rejects_invalid_suggest_with_clear_error() {
    let error = manifest(r#"{"require":{},"suggest":false}"#)
        .unwrap_err()
        .to_string();

    assert!(error.contains("suggest must be an object"));
}

#[test]
fn reads_composer_repositories() {
    let manifest = manifest(
        r#"
        {
            "repositories": [
                {"type": "composer", "url": "https://repo.example.com/"}
            ],
            "require": {
                "acme/package": "^1.0"
            }
        }
        "#,
    )
    .unwrap();

    assert_eq!(
        manifest.repositories,
        vec![ComposerRepository {
            url: "https://repo.example.com".to_string(),
        }]
    );
}

#[test]
fn rejects_unsupported_repository_types() {
    let error = manifest(
        r#"
        {
            "repositories": [
                {"type": "vcs", "url": "https://github.com/acme/package"}
            ],
            "require": {}
        }
        "#,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("repository type vcs is not supported yet"));
}

#[test]
fn rejects_invalid_package_names() {
    assert!(package_path_parts("../evil").is_err());
    assert!(package_path_parts("monolog").is_err());
    assert!(package_path_parts("monolog/monolog").is_ok());
}

#[test]
fn distinguishes_package_names_from_platform_requirements() {
    assert!(is_package_name("psr/log"));
    assert!(is_package_name("monolog/monolog"));

    assert!(!is_package_name("php"));
    assert!(!is_package_name("ext-json"));
    assert!(!is_package_name("lib-icu"));

    assert!(is_platform_requirement("php"));
    assert!(is_platform_requirement("ext-json"));
    assert!(is_platform_requirement("lib-icu"));
    assert!(!is_platform_requirement("psr/log"));
    assert!(!is_platform_requirement("psr/log-implementation"));
}
