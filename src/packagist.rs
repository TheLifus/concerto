use crate::composer::package_path_parts;

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

    let first = versions.first().ok_or_else(|| {
        format!("Packagist metadata does not contain a version for {package_name}")
    })?;

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

        let release = first_release_candidate(metadata_json, "monolog/monolog").unwrap();

        assert_eq!(release.version_count, 2);
        assert_eq!(release.version, "3.9.0");
        assert_eq!(
            release.dist_url,
            "https://api.github.com/repos/Seldaek/monolog/zipball/abc123"
        );
    }
}
