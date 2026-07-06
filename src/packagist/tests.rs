use super::*;

fn constraints(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn builds_package_url() {
    let url = package_url("monolog/monolog").unwrap();

    assert_eq!(url, "https://repo.packagist.org/p2/monolog/monolog.json");
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
    let release = first_release_candidate(metadata_json, "monolog/monolog", &constraints).unwrap();

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
    let release = first_release_candidate(metadata_json, "monolog/monolog", &constraints).unwrap();

    assert_eq!(release.version, "3.8.1");
    assert_eq!(release.dist_url, "https://example.com/3.8.1.zip");
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
    let release = first_release_candidate(metadata_json, "psr/log", &constraints).unwrap();

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
    let error = first_release_candidate(metadata_json, "psr/log", &constraints).unwrap_err();

    assert!(error.contains("psr/log"));
    assert!(error.contains("^3.0, ^2.0"));
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
    let release = first_release_candidate(metadata_json, "monolog/monolog", &constraints).unwrap();

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
