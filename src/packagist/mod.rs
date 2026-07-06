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
mod tests;
