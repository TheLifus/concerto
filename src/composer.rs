#[derive(Debug, PartialEq)]
pub struct RequiredPackage {
    pub name: String,
    pub constraint: String,
}

pub fn required_packages(composer_json: &str) -> Result<Vec<RequiredPackage>, String> {
    let parsed: serde_json::Value = serde_json::from_str(composer_json)
        .map_err(|error| format!("Invalid composer.json: {error}"))?;

    let require = parsed
        .get("require")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "composer.json require must be an object".to_string())?;

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

pub(crate) fn package_path_parts(package_name: &str) -> Result<(&str, &str), String> {
    let (vendor, package) = package_name
        .split_once('/')
        .ok_or_else(|| format!("Invalid package name: {package_name}"))?;

    if vendor.is_empty()
        || package.is_empty()
        || vendor == "."
        || vendor == ".."
        || package == "."
        || package == ".."
    {
        return Err(format!("Invalid package name: {package_name}"));
    }

    Ok((vendor, package))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_required_packages() {
        let composer_json = r#"
        {
            "require": {
                "monolog/monolog": "^3.0"
            }
        }
        "#;

        let packages = required_packages(composer_json).unwrap();

        assert_eq!(
            packages,
            vec![RequiredPackage {
                name: "monolog/monolog".to_string(),
                constraint: "^3.0".to_string(),
            }]
        );
    }

    #[test]
    fn rejects_invalid_package_names() {
        assert!(package_path_parts("../evil").is_err());
        assert!(package_path_parts("monolog").is_err());
        assert!(package_path_parts("monolog/monolog").is_ok());
    }
}
