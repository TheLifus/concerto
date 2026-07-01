use crate::composer::{
    RequiredPackage, is_package_name, package_path_parts, required_packages_from_object,
};

const NO_MATCHING_VERSION: &str = "Packagist metadata does not contain a version matching";

#[derive(Debug)]
pub struct PackagistRelease {
    pub version_count: usize,
    pub version: String,
    pub dist_url: String,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

pub fn package_url(package_name: &str) -> Result<String, String> {
    let (vendor, package) = package_path_parts(package_name)?;

    Ok(format!(
        "https://repo.packagist.org/p2/{vendor}/{package}.json"
    ))
}

pub fn first_release_candidate(
    metadata_json: &str,
    package_name: &str,
    constraints: &[String],
) -> Result<PackagistRelease, String> {
    let parsed: serde_json::Value = serde_json::from_str(metadata_json)
        .map_err(|error| format!("Invalid Packagist metadata: {error}"))?;

    let versions = parsed
        .get("packages")
        .and_then(|packages| packages.get(package_name))
        .and_then(|versions| versions.as_array())
        .ok_or_else(|| {
            format!("Packagist metadata does not contain versions for {package_name}")
        })?;

    let first = versions
        .iter()
        .find(|version| {
            version
                .get("version")
                .and_then(|version| version.as_str())
                .is_some_and(|version| {
                    constraints.iter().all(|constraint| {
                        semver_php::Semver::satisfies(version, constraint).unwrap_or(false)
                    })
                })
        })
        .ok_or_else(|| no_matching_version_error(package_name, constraints))?;

    let version = first
        .get("version")
        .and_then(|version| version.as_str())
        .ok_or_else(|| {
            format!("Packagist metadata does not contain a version for {package_name}")
        })?;

    let dist_url = first
        .get("dist")
        .and_then(|dist| dist.get("url"))
        .and_then(|url| url.as_str())
        .ok_or_else(|| {
            format!("Packagist metadata does not contain a dist url for {package_name}")
        })?;

    let (package_requires, platform_requires) = release_requirements(first)?;

    Ok(PackagistRelease {
        version_count: versions.len(),
        version: version.to_string(),
        dist_url: dist_url.to_string(),
        package_requires,
        platform_requires,
    })
}

fn release_requirements(
    release: &serde_json::Value,
) -> Result<(Vec<RequiredPackage>, Vec<RequiredPackage>), String> {
    let Some(require) = release.get("require") else {
        return Ok((Vec::new(), Vec::new()));
    };

    let require = require
        .as_object()
        .ok_or_else(|| "Packagist release require must be an object".to_string())?;

    let requirements = required_packages_from_object(require)?;

    let (package_requires, platform_requires) = requirements
        .into_iter()
        .partition(|requirement| is_package_name(&requirement.name));

    Ok((package_requires, platform_requires))
}

fn no_matching_version_error(package_name: &str, constraints: &[String]) -> String {
    format!(
        "{NO_MATCHING_VERSION} all constraints for {package_name}: {}",
        constraints.join(", ")
    )
}

#[cfg(test)]
mod tests {
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
        let release =
            first_release_candidate(metadata_json, "monolog/monolog", &constraints).unwrap();

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
        let release =
            first_release_candidate(metadata_json, "monolog/monolog", &constraints).unwrap();

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
        let release =
            first_release_candidate(metadata_json, "monolog/monolog", &constraints).unwrap();

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
}
