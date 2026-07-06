use crate::composer::{
    RequiredPackage, is_package_name, package_path_parts, required_packages_from_object,
};
use crate::error::{ConcertoError, Result};
use crate::platform::{self, Platform};

const NO_MATCHING_VERSION: &str = "Packagist metadata does not contain a version matching";

#[derive(Debug)]
pub struct PackagistRelease {
    pub version_count: usize,
    pub version: String,
    pub dist_url: String,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

struct PlatformRejection {
    version: String,
    error: String,
}

pub fn package_url(package_name: &str) -> Result<String> {
    let constraints = Vec::new();
    let (vendor, package) = package_path_parts(package_name).map_err(|error| {
        ConcertoError::resolution(package_name, &constraints, error.to_string())
    })?;

    Ok(format!(
        "https://repo.packagist.org/p2/{vendor}/{package}.json"
    ))
}

pub fn first_release_candidate(
    metadata_json: &str,
    package_name: &str,
    constraints: &[String],
    platform: &Platform,
) -> Result<PackagistRelease> {
    let parsed: serde_json::Value = serde_json::from_str(metadata_json).map_err(|error| {
        ConcertoError::resolution(
            package_name,
            constraints,
            format!("invalid Packagist metadata: {error}"),
        )
    })?;

    let versions = parsed
        .get("packages")
        .and_then(|packages| packages.get(package_name))
        .and_then(|versions| versions.as_array())
        .ok_or_else(|| {
            ConcertoError::resolution(
                package_name,
                constraints,
                "Packagist metadata does not contain versions for this package",
            )
        })?;

    let mut platform_rejections = Vec::new();

    for release in versions {
        let candidate = release_candidate(release, package_name, constraints, versions.len())?;

        if !candidate.matches_constraints(package_name, constraints)? {
            continue;
        }

        if let Err(error) = platform::validate(&candidate.platform_requires, platform, package_name)
        {
            platform_rejections.push(PlatformRejection {
                version: candidate.version.clone(),
                error: error.to_string(),
            });
            continue;
        }

        return Ok(candidate);
    }

    Err(no_matching_version_error(
        package_name,
        constraints,
        platform,
        &platform_rejections,
    ))
}

impl PackagistRelease {
    fn matches_constraints(&self, package_name: &str, constraints: &[String]) -> Result<bool> {
        for constraint in constraints {
            let matches =
                semver_php::Semver::satisfies(&self.version, constraint).map_err(|error| {
                    ConcertoError::resolution(
                        package_name,
                        constraints,
                        format!("could not check package constraint: {error}"),
                    )
                })?;

            if !matches {
                return Ok(false);
            }
        }

        Ok(true)
    }
}

fn release_candidate(
    release: &serde_json::Value,
    package_name: &str,
    constraints: &[String],
    version_count: usize,
) -> Result<PackagistRelease> {
    let version = release
        .get("version")
        .and_then(|version| version.as_str())
        .ok_or_else(|| {
            ConcertoError::resolution(
                package_name,
                constraints,
                "Packagist metadata does not contain a version for this release",
            )
        })?;

    let dist_url = release
        .get("dist")
        .and_then(|dist| dist.get("url"))
        .and_then(|url| url.as_str())
        .ok_or_else(|| {
            ConcertoError::resolution(
                package_name,
                constraints,
                "Packagist metadata does not contain a dist url for this release",
            )
        })?;

    let (package_requires, platform_requires) =
        release_requirements(release, package_name, constraints)?;

    Ok(PackagistRelease {
        version_count,
        version: version.to_string(),
        dist_url: dist_url.to_string(),
        package_requires,
        platform_requires,
    })
}

fn release_requirements(
    release: &serde_json::Value,
    package_name: &str,
    constraints: &[String],
) -> Result<(Vec<RequiredPackage>, Vec<RequiredPackage>)> {
    let Some(require) = release.get("require") else {
        return Ok((Vec::new(), Vec::new()));
    };

    let require = require.as_object().ok_or_else(|| {
        ConcertoError::resolution(
            package_name,
            constraints,
            "Packagist release require must be an object",
        )
    })?;

    let requirements = required_packages_from_object(require)
        .map_err(|error| ConcertoError::resolution(package_name, constraints, error.to_string()))?;

    let (package_requires, platform_requires) = requirements
        .into_iter()
        .partition(|requirement| is_package_name(&requirement.name));

    Ok((package_requires, platform_requires))
}

fn no_matching_version_error(
    package_name: &str,
    constraints: &[String],
    platform: &Platform,
    platform_rejections: &[PlatformRejection],
) -> ConcertoError {
    let mut error = format!(
        "{} all constraints and current platform for {}: {} ({})",
        NO_MATCHING_VERSION,
        package_name,
        constraints.join(", "),
        platform_summary(platform)
    );

    if !platform_rejections.is_empty() {
        error.push_str(". Skipped platform-incompatible versions: ");
        error.push_str(
            &platform_rejections
                .iter()
                .map(|rejection| format!("{} ({})", rejection.version, rejection.error))
                .collect::<Vec<_>>()
                .join("; "),
        );
    }

    ConcertoError::resolution(package_name, constraints, error)
}

fn platform_summary(platform: &Platform) -> String {
    if platform.extensions.is_empty() {
        return format!("php {}, no extensions detected", platform.php_version);
    }

    format!(
        "php {}, extensions: {}",
        platform.php_version,
        platform.extensions.join(", ")
    )
}

#[cfg(test)]
mod tests;
