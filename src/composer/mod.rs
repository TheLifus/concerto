use crate::error::{ConcertoError, Result};

pub(crate) const REQUIRE_MUST_BE_OBJECT: &str = "composer.json require must be an object";

const INVALID_COMPOSER_JSON: &str = "Invalid composer.json";

#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequiredPackage {
    pub name: String,
    pub constraint: String,
}

pub fn required_packages(composer_json: &str) -> Result<Vec<RequiredPackage>> {
    let parsed: serde_json::Value = serde_json::from_str(composer_json).map_err(|error| {
        ConcertoError::composer_json(format!("{INVALID_COMPOSER_JSON}: {error}"))
    })?;

    let require = parsed
        .get("require")
        .and_then(|value| value.as_object())
        .ok_or_else(|| ConcertoError::composer_json(REQUIRE_MUST_BE_OBJECT))?;

    required_packages_from_object(require)
        .map_err(|error| ConcertoError::composer_json(error.to_string()))
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
