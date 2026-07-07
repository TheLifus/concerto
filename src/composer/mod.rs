use crate::error::{ConcertoError, Result};

pub(crate) const REQUIRE_MUST_BE_OBJECT: &str = "composer.json require must be an object";

const INVALID_COMPOSER_JSON: &str = "Invalid composer.json";

#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequiredPackage {
    pub name: String,
    pub constraint: String,
}

#[derive(Debug)]
pub(crate) struct ComposerManifest {
    pub package_requirements: Vec<RequiredPackage>,
    pub platform_requirements: Vec<RequiredPackage>,
    pub require_dev: Vec<RequiredPackage>,
    pub repositories: Vec<ComposerRepository>,
}

#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ComposerRepository {
    pub url: String,
}

#[cfg(test)]
pub(crate) fn required_packages(composer_json: &str) -> Result<Vec<RequiredPackage>> {
    Ok(manifest(composer_json)?.package_requirements)
}

pub(crate) fn manifest(composer_json: &str) -> Result<ComposerManifest> {
    let parsed: serde_json::Value = serde_json::from_str(composer_json).map_err(|error| {
        ConcertoError::composer_json(format!("{INVALID_COMPOSER_JSON}: {error}"))
    })?;

    reject_unsupported_sections(&parsed)?;

    let require = parsed
        .get("require")
        .and_then(|value| value.as_object())
        .ok_or_else(|| ConcertoError::composer_json(REQUIRE_MUST_BE_OBJECT))?;

    let requirements = required_packages_from_object(require)
        .map_err(|error| ConcertoError::composer_json(error.to_string()))?;
    let (platform_requirements, package_requirements) = split_platform_requirements(requirements);
    let require_dev = optional_required_packages(&parsed, "require-dev")?;
    let repositories = repositories(&parsed)?;

    Ok(ComposerManifest {
        package_requirements,
        platform_requirements,
        require_dev,
        repositories,
    })
}

impl ComposerManifest {
    pub(crate) fn root_requirements(&self, include_dev: bool) -> Vec<RequiredPackage> {
        self.package_requirements
            .iter()
            .chain(&self.platform_requirements)
            .chain(
                include_dev
                    .then_some(&self.require_dev)
                    .into_iter()
                    .flatten(),
            )
            .cloned()
            .collect()
    }

    pub(crate) fn install_requirements(&self, include_dev: bool) -> Vec<RequiredPackage> {
        self.package_requirements
            .iter()
            .chain(
                include_dev
                    .then_some(&self.require_dev)
                    .into_iter()
                    .flatten(),
            )
            .cloned()
            .collect()
    }
}

fn optional_required_packages(
    parsed: &serde_json::Value,
    section: &str,
) -> Result<Vec<RequiredPackage>> {
    let Some(value) = parsed.get(section) else {
        return Ok(Vec::new());
    };

    let object = value.as_object().ok_or_else(|| {
        ConcertoError::composer_json(format!("composer.json {section} must be an object"))
    })?;

    required_packages_from_object(object)
        .map_err(|error| ConcertoError::composer_json(error.to_string()))
}

fn split_platform_requirements(
    requirements: Vec<RequiredPackage>,
) -> (Vec<RequiredPackage>, Vec<RequiredPackage>) {
    requirements
        .into_iter()
        .partition(|requirement| is_platform_requirement(&requirement.name))
}

fn reject_unsupported_sections(parsed: &serde_json::Value) -> Result<()> {
    if parsed
        .get("suggest")
        .is_some_and(|suggest| !suggest.is_object())
    {
        return Err(ConcertoError::composer_json(
            "composer.json suggest must be an object",
        ));
    }

    if parsed.get("scripts").is_some() {
        return Err(ConcertoError::composer_json(
            "composer.json scripts are not supported yet",
        ));
    }

    if parsed
        .get("config")
        .and_then(|config| config.get("allow-plugins"))
        .is_some()
    {
        return Err(ConcertoError::composer_json(
            "Composer plugins are not supported yet",
        ));
    }

    Ok(())
}

fn repositories(parsed: &serde_json::Value) -> Result<Vec<ComposerRepository>> {
    let Some(value) = parsed.get("repositories") else {
        return Ok(Vec::new());
    };

    let repositories = value.as_array().ok_or_else(|| {
        ConcertoError::composer_json("composer.json repositories must be an array")
    })?;

    repositories
        .iter()
        .map(|repository| {
            let repository_type = repository
                .get("type")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    ConcertoError::composer_json("composer repository type must be a string")
                })?;

            if repository_type != "composer" {
                return Err(ConcertoError::composer_json(format!(
                    "composer repository type {repository_type} is not supported yet"
                )));
            }

            let url = repository
                .get("url")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    ConcertoError::composer_json("composer repository url must be a string")
                })?;

            Ok(ComposerRepository {
                url: url.trim_end_matches('/').to_string(),
            })
        })
        .collect()
}

pub(crate) fn package_path_parts(package_name: &str) -> Result<(&str, &str)> {
    let (vendor, package) = package_name
        .split_once('/')
        .ok_or_else(|| ConcertoError::invalid_package_name(package_name))?;

    if vendor.is_empty()
        || package.is_empty()
        || vendor == "."
        || vendor == ".."
        || package == "."
        || package == ".."
    {
        return Err(ConcertoError::invalid_package_name(package_name));
    }

    Ok((vendor, package))
}

pub(crate) fn is_package_name(name: &str) -> bool {
    package_path_parts(name).is_ok()
}

pub(crate) fn is_platform_requirement(name: &str) -> bool {
    let name = name.to_lowercase();

    name == "php" || name.starts_with("ext-") || name.starts_with("lib-")
}

pub(crate) fn required_packages_from_object(
    require: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<RequiredPackage>> {
    require
        .iter()
        .map(|(package, constraint)| {
            let constraint = constraint.as_str().ok_or_else(|| {
                ConcertoError::requirement(format!(
                    "package constraint for {package} must be a string"
                ))
            })?;

            Ok(RequiredPackage {
                name: package.to_string(),
                constraint: constraint.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
