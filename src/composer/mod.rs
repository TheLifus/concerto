pub(crate) const REQUIRE_MUST_BE_OBJECT: &str = "composer.json require must be an object";

const INVALID_COMPOSER_JSON: &str = "Invalid composer.json";
const INVALID_PACKAGE_NAME: &str = "Invalid package name";

#[derive(Debug, PartialEq, Eq, Clone, serde::Serialize, serde::Deserialize)]
pub struct RequiredPackage {
    pub name: String,
    pub constraint: String,
}

pub fn required_packages(composer_json: &str) -> Result<Vec<RequiredPackage>, String> {
    let parsed: serde_json::Value = serde_json::from_str(composer_json)
        .map_err(|error| format!("{INVALID_COMPOSER_JSON}: {error}"))?;

    let require = parsed
        .get("require")
        .and_then(|value| value.as_object())
        .ok_or_else(|| REQUIRE_MUST_BE_OBJECT.to_string())?;

    required_packages_from_object(require)
}

pub(crate) fn package_path_parts(package_name: &str) -> Result<(&str, &str), String> {
    let (vendor, package) = package_name
        .split_once('/')
        .ok_or_else(|| format!("{INVALID_PACKAGE_NAME}: {package_name}"))?;

    if vendor.is_empty()
        || package.is_empty()
        || vendor == "."
        || vendor == ".."
        || package == "."
        || package == ".."
    {
        return Err(format!("{INVALID_PACKAGE_NAME}: {package_name}"));
    }

    Ok((vendor, package))
}

pub(crate) fn is_package_name(name: &str) -> bool {
    package_path_parts(name).is_ok()
}

pub(crate) fn required_packages_from_object(
    require: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<RequiredPackage>, String> {
    require
        .iter()
        .map(|(package, constraint)| {
            let constraint = constraint
                .as_str()
                .ok_or_else(|| format!("package constraint for {package} must be a string"))?;

            Ok(RequiredPackage {
                name: package.to_string(),
                constraint: constraint.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
