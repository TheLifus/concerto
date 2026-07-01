use crate::composer::package_path_parts;

const NO_MATCHING_VERSION: &str = "Packagist metadata does not contain a version matching";

pub struct PackagistRelease {
    pub version_count: usize,
    pub version: String,
    pub dist_url: String,
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
    constraint: &str,
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
                    semver_php::Semver::satisfies(version, constraint).unwrap_or(false)
                })
        })
        .ok_or_else(|| no_matching_version_error(package_name, constraint))?;

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

    Ok(PackagistRelease {
        version_count: versions.len(),
        version: version.to_string(),
        dist_url: dist_url.to_string(),
    })
}

fn no_matching_version_error(package_name: &str, constraint: &str) -> String {
    format!("{NO_MATCHING_VERSION} {constraint} for {package_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let release = first_release_candidate(metadata_json, "monolog/monolog", "^3.0").unwrap();

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

        let release = first_release_candidate(metadata_json, "monolog/monolog", "^3.0").unwrap();

        assert_eq!(release.version, "3.8.1");
        assert_eq!(release.dist_url, "https://example.com/3.8.1.zip");
    }
}
