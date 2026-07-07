use super::*;
use crate::platform::Platform;
use std::collections::HashMap;

fn constraints(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
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

#[test]
fn builds_package_url() {
    let url = package_url("monolog/monolog").unwrap();

    assert_eq!(url, "https://repo.packagist.org/p2/monolog/monolog.json");
}

#[test]
fn builds_repository_package_url() {
    let url = repository_package_url("https://repo.example.com", "monolog/monolog").unwrap();

    assert_eq!(url, "https://repo.example.com/p2/monolog/monolog.json");
}

#[test]
fn builds_provider_url() {
    let url = providers_url("psr/log-implementation").unwrap();

    assert_eq!(
        url,
        "https://packagist.org/providers/psr/log-implementation.json"
    );
}

#[test]
fn reads_provider_names() {
    let metadata_json = r#"
{
    "providers": [
        { "name": "acme/logger" },
        { "name": "acme/logger" },
        { "name": "monolog/monolog" }
    ]
}
"#;
    let names = provider_names(
        metadata_json,
        "psr/log-implementation",
        &constraints(&["^1.0"]),
    )
    .unwrap();

    assert_eq!(
        names,
        vec!["acme/logger".to_string(), "monolog/monolog".to_string()]
    );
}

#[test]
fn reads_first_release_candidate() {
    let metadata_json = r#"
    {
        "packages": {
            "monolog/monolog": [
                {
                    "version": "3.9.0",
                    "dist": {
                        "url": "https://api.github.com/repos/Seldaek/monolog/zipball/abc123"
                    }
                },
                {
                    "version": "3.8.1",
                    "dist": {
                        "url": "https://api.github.com/repos/Seldaek/monolog/zipball/def456"
                    }
                }
            ]
        }
    }
    "#;

    let constraints = constraints(&["^3.0"]);
    let release = first_release_candidate(
        metadata_json,
        "monolog/monolog",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(release.version_count, 2);
    assert_eq!(release.version, "3.9.0");
    assert_eq!(
        release.dist_url,
        "https://api.github.com/repos/Seldaek/monolog/zipball/abc123"
    );
}

#[test]
fn semver_php_matches_composer_constraints() {
    assert!(semver_php::Semver::satisfies("3.10.0", "^3.0").unwrap());
    assert!(semver_php::Semver::satisfies("3.0.2", "^3.0").unwrap());
    assert!(!semver_php::Semver::satisfies("2.9.0", "^3.0").unwrap());
}

#[test]
fn skips_releases_that_do_not_match_constraint() {
    let metadata_json = r#"
{
    "packages": {
        "monolog/monolog": [
            {
                "version": "2.9.0",
                "dist": {
                    "url": "https://example.com/2.9.0.zip"
                }
            },
            {
                "version": "3.8.1",
                "dist": {
                    "url": "https://example.com/3.8.1.zip"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^3.0"]);
    let release = first_release_candidate(
        metadata_json,
        "monolog/monolog",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "3.8.1");
    assert_eq!(release.dist_url, "https://example.com/3.8.1.zip");
}

#[test]
fn skips_unset_require_on_release_that_does_not_match_constraint() {
    let metadata_json = r#"
{
    "packages": {
        "psr/log": [
            {
                "version": "3.0.2",
                "dist": {
                    "url": "https://example.com/3.0.2.zip"
                },
                "require": {
                    "php": ">=8.0"
                }
            },
            {
                "version": "1.0.0",
                "dist": {
                    "url": "https://example.com/1.0.0.zip"
                },
                "require": "__unset"
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^3.0"]);
    let release = first_release_candidate(
        metadata_json,
        "psr/log",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "3.0.2");
}

#[test]
fn does_not_parse_require_for_release_that_does_not_match_constraint() {
    let metadata_json = r#"
{
    "packages": {
        "psr/log": [
            {
                "version": "3.0.2",
                "dist": {
                    "url": "https://example.com/3.0.2.zip"
                },
                "require": {
                    "php": ">=8.0"
                }
            },
            {
                "version": "1.0.0",
                "dist": {
                    "url": "https://example.com/1.0.0.zip"
                },
                "require": {
                    "php": false
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^3.0"]);
    let release = first_release_candidate(
        metadata_json,
        "psr/log",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "3.0.2");
}

#[test]
fn inherits_missing_release_require_from_previous_packagist_entry() {
    let metadata_json = r#"
{
    "packages": {
        "symfony/console": [
            {
                "version": "7.4.14",
                "dist": {
                    "url": "https://example.com/7.4.14.zip"
                },
                "require": {
                    "php": ">=8.2",
                    "symfony/string": "^7.2|^8.0"
                }
            },
            {
                "version": "7.4.13",
                "dist": {
                    "url": "https://example.com/7.4.13.zip"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["7.4.13"]);
    let release = first_release_candidate(
        metadata_json,
        "symfony/console",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "7.4.13");
    assert_eq!(
        release.package_requires,
        vec![RequiredPackage {
            name: "symfony/string".to_string(),
            constraint: "^7.2|^8.0".to_string(),
        }]
    );
}

#[test]
fn treats_unset_require_as_empty_requirements() {
    let metadata_json = r#"
{
    "packages": {
        "psr/log": [
            {
                "version": "1.0.0",
                "dist": {
                    "url": "https://example.com/1.0.0.zip"
                },
                "require": "__unset"
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^1.0"]);
    let release = first_release_candidate(
        metadata_json,
        "psr/log",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "1.0.0");
    assert_eq!(release.package_requires, Vec::new());
    assert_eq!(release.platform_requires, Vec::new());
}

#[test]
fn reads_conflict_provide_and_replace_links() {
    let metadata_json = r#"
{
    "packages": {
        "acme/logger": [
            {
                "version": "1.0.0",
                "dist": {
                    "url": "https://example.com/1.0.0.zip"
                },
                "conflict": {
                    "acme/broken": "<2.0"
                },
                "provide": {
                    "psr/log-implementation": "1.0.0"
                },
                "replace": {
                    "acme/old-logger": "self.version"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^1.0"]);
    let release = first_release_candidate(
        metadata_json,
        "acme/logger",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(
        release.conflicts,
        vec![RequiredPackage {
            name: "acme/broken".to_string(),
            constraint: "<2.0".to_string(),
        }]
    );
    assert_eq!(
        release.provides,
        vec![RequiredPackage {
            name: "psr/log-implementation".to_string(),
            constraint: "1.0.0".to_string(),
        }]
    );
    assert_eq!(
        release.replaces,
        vec![RequiredPackage {
            name: "acme/old-logger".to_string(),
            constraint: "self.version".to_string(),
        }]
    );
}

#[test]
fn skips_releases_without_dist_url() {
    let metadata_json = r#"
{
    "packages": {
        "symfony/console": [
            {
                "version": "7.4.0"
            },
            {
                "version": "7.3.0",
                "dist": {
                    "url": "https://example.com/7.3.0.zip"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^7.0"]);
    let release = first_release_candidate(
        metadata_json,
        "symfony/console",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "7.3.0");
    assert_eq!(release.dist_url, "https://example.com/7.3.0.zip");
}

#[test]
fn skips_releases_with_unmet_php_requirement() {
    let metadata_json = r#"
{
    "packages": {
        "symfony/console": [
            {
                "version": "7.4.0",
                "dist": {
                    "url": "https://example.com/7.4.0.zip"
                },
                "require": {
                    "php": ">=8.4"
                }
            },
            {
                "version": "7.3.0",
                "dist": {
                    "url": "https://example.com/7.3.0.zip"
                },
                "require": {
                    "php": ">=8.2"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^7.0"]);
    let release = first_release_candidate(
        metadata_json,
        "symfony/console",
        &constraints,
        &platform("8.2.25", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "7.3.0");
    assert_eq!(release.dist_url, "https://example.com/7.3.0.zip");
}

#[test]
fn skips_releases_with_missing_extension_requirement() {
    let metadata_json = r#"
{
    "packages": {
        "acme/package": [
            {
                "version": "1.1.0",
                "dist": {
                    "url": "https://example.com/1.1.0.zip"
                },
                "require": {
                    "ext-intl": "*"
                }
            },
            {
                "version": "1.0.0",
                "dist": {
                    "url": "https://example.com/1.0.0.zip"
                },
                "require": {
                    "ext-json": "*"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^1.0"]);
    let release = first_release_candidate(
        metadata_json,
        "acme/package",
        &constraints,
        &platform("8.2.25", &["json"]),
    )
    .unwrap();

    assert_eq!(release.version, "1.0.0");
    assert_eq!(release.dist_url, "https://example.com/1.0.0.zip");
}

#[test]
fn skips_releases_with_unsupported_library_requirement() {
    let metadata_json = r#"
{
    "packages": {
        "acme/package": [
            {
                "version": "1.1.0",
                "dist": {
                    "url": "https://example.com/1.1.0.zip"
                },
                "require": {
                    "lib-icu": "*"
                }
            },
            {
                "version": "1.0.0",
                "dist": {
                    "url": "https://example.com/1.0.0.zip"
                },
                "require": "__unset"
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^1.0"]);
    let release = first_release_candidate(
        metadata_json,
        "acme/package",
        &constraints,
        &platform("8.2.25", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "1.0.0");
    assert_eq!(release.dist_url, "https://example.com/1.0.0.zip");
}

#[test]
fn rejects_when_no_release_matches_platform() {
    let metadata_json = r#"
{
    "packages": {
        "symfony/console": [
            {
                "version": "7.4.0",
                "dist": {
                    "url": "https://example.com/7.4.0.zip"
                },
                "require": {
                    "php": ">=8.4"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^7.0"]);
    let error = first_release_candidate(
        metadata_json,
        "symfony/console",
        &constraints,
        &platform("8.2.25", &[]),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("symfony/console"));
    assert!(error.contains("^7.0"));
    assert!(error.contains("current platform"));
    assert!(error.contains("php 8.2.25"));
    assert!(error.contains("7.4.0"));
    assert!(error.contains("php >=8.4 required, detected 8.2.25"));
}

#[test]
fn selects_release_matching_all_constraints() {
    let metadata_json = r#"
{
    "packages": {
        "psr/log": [
            {
                "version": "3.0.2",
                "dist": {
                    "url": "https://example.com/3.0.2.zip"
                }
            },
            {
                "version": "2.0.0",
                "dist": {
                    "url": "https://example.com/2.0.0.zip"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^2.0 || ^3.0", "^2.0"]);
    let release = first_release_candidate(
        metadata_json,
        "psr/log",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(release.version, "2.0.0");
    assert_eq!(release.dist_url, "https://example.com/2.0.0.zip");
}

#[test]
fn rejects_when_no_release_matches_all_constraints() {
    let metadata_json = r#"
{
    "packages": {
        "psr/log": [
            {
                "version": "3.0.2",
                "dist": {
                    "url": "https://example.com/3.0.2.zip"
                }
            },
            {
                "version": "2.0.0",
                "dist": {
                    "url": "https://example.com/2.0.0.zip"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^3.0", "^2.0"]);
    let error = first_release_candidate(
        metadata_json,
        "psr/log",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("psr/log"));
    assert!(error.contains("^3.0, ^2.0"));
}

#[test]
fn reports_invalid_packagist_require_without_composer_json_hint() {
    let metadata_json = r#"
{
    "packages": {
        "acme/package": [
            {
                "version": "1.0.0",
                "dist": {
                    "url": "https://example.com/1.0.0.zip"
                },
                "require": {
                    "psr/log": false
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^1.0"]);
    let error = first_release_candidate(
        metadata_json,
        "acme/package",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("acme/package"));
    assert!(error.contains("package constraint for psr/log must be a string"));
    assert!(!error.contains("Check composer.json"));
}

#[test]
fn reads_release_requirements() {
    let metadata_json = r#"
{
    "packages": {
        "monolog/monolog": [
            {
                "version": "3.9.0",
                "dist": {
                    "url": "https://example.com/3.9.0.zip"
                },
                "require": {
                    "php": ">=8.1",
                    "psr/log": "^2.0 || ^3.0"
                }
            }
        ]
    }
}
"#;

    let constraints = constraints(&["^3.0"]);
    let release = first_release_candidate(
        metadata_json,
        "monolog/monolog",
        &constraints,
        &platform("8.2.0", &[]),
    )
    .unwrap();

    assert_eq!(
        release.package_requires,
        vec![RequiredPackage {
            name: "psr/log".to_string(),
            constraint: "^2.0 || ^3.0".to_string(),
        }]
    );

    assert_eq!(
        release.platform_requires,
        vec![RequiredPackage {
            name: "php".to_string(),
            constraint: ">=8.1".to_string(),
        }]
    );
}
