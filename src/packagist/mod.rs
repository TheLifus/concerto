use crate::composer::{
    RequiredPackage, is_platform_requirement, package_path_parts, required_packages_from_object,
};
use crate::error::{ConcertoError, Result};
use crate::platform::{self, Platform};

const NO_MATCHING_VERSION: &str = "Packagist metadata does not contain a version matching";

#[derive(Clone, Debug)]
pub struct PackagistRelease {
    pub version_count: usize,
    pub version: String,
    pub dist_url: String,
    pub dist_shasum: Option<String>,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
    pub conflicts: Vec<RequiredPackage>,
    pub provides: Vec<RequiredPackage>,
    pub replaces: Vec<RequiredPackage>,
}

struct PlatformRejection {
    version: String,
    error: String,
}

pub fn package_url(package_name: &str) -> Result<String> {
    repository_package_url("https://repo.packagist.org", package_name)
}

pub(crate) fn providers_url(package_name: &str) -> Result<String> {
    repository_providers_url("https://packagist.org", package_name)
}

pub(crate) fn repository_package_url(repository_url: &str, package_name: &str) -> Result<String> {
    let constraints = Vec::new();
    let (vendor, package) = package_path_parts(package_name).map_err(|error| {
        ConcertoError::resolution(package_name, &constraints, error.to_string())
    })?;

    Ok(format!("{repository_url}/p2/{vendor}/{package}.json"))
}

pub(crate) fn repository_providers_url(repository_url: &str, package_name: &str) -> Result<String> {
    if package_name.is_empty() || package_name.contains("..") {
        return Err(ConcertoError::invalid_package_name(package_name));
    }

    Ok(format!("{repository_url}/providers/{package_name}.json"))
}

pub(crate) fn provider_names(
    metadata_json: &str,
    package_name: &str,
    constraints: &[String],
) -> Result<Vec<String>> {
    let parsed: serde_json::Value = serde_json::from_str(metadata_json).map_err(|error| {
        ConcertoError::resolution(
            package_name,
            constraints,
            format!("invalid Packagist provider metadata: {error}"),
        )
    })?;
    let providers = parsed
        .get("providers")
        .and_then(|providers| providers.as_array())
        .ok_or_else(|| {
            ConcertoError::resolution(
                package_name,
                constraints,
                "Packagist provider metadata does not contain providers",
            )
        })?;
    let mut names = providers
        .iter()
        .map(|provider| {
            provider
                .get("name")
                .and_then(|name| name.as_str())
                .map(str::to_string)
                .ok_or_else(|| {
                    ConcertoError::resolution(
                        package_name,
                        constraints,
                        "Packagist provider entry does not contain a package name",
                    )
                })
        })
        .collect::<Result<Vec<_>>>()?;

    names.sort();
    names.dedup();

    Ok(names)
}

#[cfg(test)]
pub(crate) fn first_release_candidate(
    metadata_json: &str,
    package_name: &str,
    constraints: &[String],
    platform: &Platform,
) -> Result<PackagistRelease> {
    let (candidates, platform_rejections) =
        parsed_release_candidates(metadata_json, package_name, constraints, platform)?;

    for candidate in candidates {
        if candidate.matches_constraints(package_name, constraints)? {
            return Ok(candidate);
        }
    }

    Err(no_matching_version_error(
        package_name,
        constraints,
        platform,
        &platform_rejections,
    ))
}

pub(crate) fn release_candidates(
    metadata_json: &str,
    package_name: &str,
    constraints: &[String],
    platform: &Platform,
) -> Result<Vec<PackagistRelease>> {
    let (candidates, platform_rejections) =
        parsed_release_candidates(metadata_json, package_name, constraints, platform)?;

    if candidates.is_empty() && !platform_rejections.is_empty() {
        return Err(no_matching_version_error(
            package_name,
            constraints,
            platform,
            &platform_rejections,
        ));
    }

    Ok(candidates)
}

fn parsed_release_candidates(
    metadata_json: &str,
    package_name: &str,
    constraints: &[String],
    platform: &Platform,
) -> Result<(Vec<PackagistRelease>, Vec<PlatformRejection>)> {
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
    let mut candidates = Vec::new();
    let mut inherited_links = InheritedLinks::default();

    for release in versions {
        inherited_links.update(release);

        let Some(candidate) = release_candidate(
            release,
            &inherited_links,
            package_name,
            constraints,
            versions.len(),
        )?
        else {
            continue;
        };

        if let Err(error) = platform::validate(&candidate.platform_requires, platform, package_name)
        {
            platform_rejections.push(PlatformRejection {
                version: candidate.version.clone(),
                error: error.to_string(),
            });
            continue;
        }

        candidates.push(candidate);
    }

    Ok((candidates, platform_rejections))
}

#[derive(Default)]
struct InheritedLinks {
    require: Option<serde_json::Value>,
    conflict: Option<serde_json::Value>,
    provide: Option<serde_json::Value>,
    replace: Option<serde_json::Value>,
}

impl InheritedLinks {
    fn update(&mut self, release: &serde_json::Value) {
        update_inherited_link(&mut self.require, release, "require");
        update_inherited_link(&mut self.conflict, release, "conflict");
        update_inherited_link(&mut self.provide, release, "provide");
        update_inherited_link(&mut self.replace, release, "replace");
    }
}

fn update_inherited_link(
    inherited: &mut Option<serde_json::Value>,
    release: &serde_json::Value,
    section: &str,
) {
    if let Some(value) = release.get(section) {
        *inherited = Some(value.clone());
    }
}

impl PackagistRelease {
    pub(crate) fn matches_constraints(
        &self,
        package_name: &str,
        constraints: &[String],
    ) -> Result<bool> {
        version_matches_constraints(&self.version, package_name, constraints)
    }
}

fn version_matches_constraints(
    version: &str,
    package_name: &str,
    constraints: &[String],
) -> Result<bool> {
    for constraint in constraints {
        let matches = semver_php::Semver::satisfies(version, constraint).map_err(|error| {
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

fn release_candidate(
    release: &serde_json::Value,
    inherited_links: &InheritedLinks,
    package_name: &str,
    constraints: &[String],
    version_count: usize,
) -> Result<Option<PackagistRelease>> {
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

    if !version_matches_constraints(version, package_name, constraints)? {
        return Ok(None);
    }

    let Some(dist_url) = release
        .get("dist")
        .and_then(|dist| dist.get("url"))
        .and_then(|url| url.as_str())
    else {
        return Ok(None);
    };
    let dist_shasum = release
        .get("dist")
        .and_then(|dist| dist.get("shasum"))
        .and_then(|shasum| shasum.as_str())
        .filter(|shasum| !shasum.is_empty())
        .map(str::to_string);

    let (package_requires, platform_requires) =
        release_requirements(inherited_links.require.as_ref(), package_name, constraints)?;
    let conflicts = release_link_section(
        inherited_links.conflict.as_ref(),
        "conflict",
        package_name,
        constraints,
    )?;
    let provides = release_link_section(
        inherited_links.provide.as_ref(),
        "provide",
        package_name,
        constraints,
    )?;
    let replaces = release_link_section(
        inherited_links.replace.as_ref(),
        "replace",
        package_name,
        constraints,
    )?;

    Ok(Some(PackagistRelease {
        version_count,
        version: version.to_string(),
        dist_url: dist_url.to_string(),
        dist_shasum,
        package_requires,
        platform_requires,
        conflicts,
        provides,
        replaces,
    }))
}

fn release_requirements(
    require: Option<&serde_json::Value>,
    package_name: &str,
    constraints: &[String],
) -> Result<(Vec<RequiredPackage>, Vec<RequiredPackage>)> {
    let Some(require) = require else {
        return Ok((Vec::new(), Vec::new()));
    };

    if require.as_str().is_some_and(|value| value == "__unset") {
        return Ok((Vec::new(), Vec::new()));
    }

    let require = require.as_object().ok_or_else(|| {
        ConcertoError::resolution(
            package_name,
            constraints,
            "Packagist release require must be an object",
        )
    })?;

    let requirements = required_packages_from_object(require)
        .map_err(|error| ConcertoError::resolution(package_name, constraints, error.to_string()))?;

    let (platform_requires, package_requires) = requirements
        .into_iter()
        .partition(|requirement| is_platform_requirement(&requirement.name));

    Ok((package_requires, platform_requires))
}

fn release_link_section(
    value: Option<&serde_json::Value>,
    section: &str,
    package_name: &str,
    constraints: &[String],
) -> Result<Vec<RequiredPackage>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    if value.as_str().is_some_and(|value| value == "__unset") {
        return Ok(Vec::new());
    }

    let links = value.as_object().ok_or_else(|| {
        ConcertoError::resolution(
            package_name,
            constraints,
            format!("Packagist release {section} must be an object"),
        )
    })?;

    required_packages_from_object(links)
        .map_err(|error| ConcertoError::resolution(package_name, constraints, error.to_string()))
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
