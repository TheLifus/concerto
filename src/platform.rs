use crate::composer::RequiredPackage;
use std::process::Command;

#[derive(Debug)]
pub(crate) struct Platform {
    pub php_version: String,
    pub extensions: Vec<String>,
}

pub(crate) fn validate(
    requirements: &[RequiredPackage],
    platform: &Platform,
    package_name: &str,
) -> Result<(), String> {
    for requirement in requirements {
        let requirement_name = requirement.name.to_lowercase();

        if requirement_name == "php" {
            validate_php_requirement(requirement, platform, package_name)?;
        } else if requirement_name.starts_with("ext-") {
            validate_extension_requirement(requirement, platform, package_name)?;
        } else if requirement_name.starts_with("lib-") {
            return Err(platform_error(package_name, requirement, "unsupported"));
        }
    }

    Ok(())
}

fn validate_php_requirement(
    requirement: &RequiredPackage,
    platform: &Platform,
    package_name: &str,
) -> Result<(), String> {
    let matches = semver_php::Semver::satisfies(&platform.php_version, &requirement.constraint)
        .map_err(|error| format!("Could not check php requirement: {error}"))?;

    if matches {
        return Ok(());
    }

    Err(platform_error(
        package_name,
        requirement,
        &platform.php_version,
    ))
}

fn validate_extension_requirement(
    requirement: &RequiredPackage,
    platform: &Platform,
    package_name: &str,
) -> Result<(), String> {
    let requirement_name = requirement.name.to_lowercase();
    let extension = requirement_name.trim_start_matches("ext-");

    if platform
        .extensions
        .iter()
        .any(|installed| installed == extension)
    {
        return Ok(());
    }

    Err(platform_error(package_name, requirement, "missing"))
}

fn platform_error(package_name: &str, requirement: &RequiredPackage, detected: &str) -> String {
    format!(
        "{package_name}: {} {} required, detected {detected}",
        requirement.name, requirement.constraint
    )
}

pub(crate) fn current() -> Result<Platform, String> {
    let php_version = command_output("php", &["-r", "echo PHP_VERSION;"])?;
    let extensions = parse_extensions(&command_output("php", &["-m"])?);

    Ok(Platform {
        php_version: php_version.trim().to_string(),
        extensions,
    })
}

fn command_output(command: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(arguments)
        .output()
        .map_err(|error| format!("Could not run {command}: {error}"))?;

    if !output.status.success() {
        return Err(format!("{command} exited with {}", output.status));
    }

    String::from_utf8(output.stdout)
        .map_err(|error| format!("{command} output is not valid UTF-8: {error}"))
}

fn parse_extensions(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('['))
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_matching_php_requirement() {
        let requirements = vec![required_package("php", ">=8.1")];
        let platform = platform("8.3.0", &[]);

        let result = validate(&requirements, &platform, "monolog/monolog");

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_unmet_php_requirement() {
        let requirements = vec![required_package("php", ">=8.4")];
        let platform = platform("8.3.0", &[]);

        let error = validate(&requirements, &platform, "symfony/console").unwrap_err();

        assert!(error.contains("symfony/console"));
        assert!(error.contains("php"));
        assert!(error.contains(">=8.4"));
        assert!(error.contains("8.3.0"));
    }

    #[test]
    fn accepts_installed_extension_requirement() {
        let requirements = vec![required_package("ext-json", "*")];
        let platform = platform("8.3.0", &["json"]);

        let result = validate(&requirements, &platform, "symfony/console");

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_missing_extension_requirement() {
        let requirements = vec![required_package("ext-intl", "*")];
        let platform = platform("8.3.0", &["json"]);

        let error = validate(&requirements, &platform, "symfony/console").unwrap_err();

        assert!(error.contains("symfony/console"));
        assert!(error.contains("ext-intl"));
        assert!(error.contains("*"));
        assert!(error.contains("missing"));
    }

    #[test]
    fn rejects_library_requirement_as_unsupported() {
        let requirements = vec![required_package("lib-icu", ">=70")];
        let platform = platform("8.3.0", &[]);

        let error = validate(&requirements, &platform, "symfony/intl").unwrap_err();

        assert!(error.contains("symfony/intl"));
        assert!(error.contains("lib-icu"));
        assert!(error.contains(">=70"));
        assert!(error.contains("unsupported"));
    }

    #[test]
    fn parses_php_extensions() {
        let extensions = parse_extensions(
            r#"
                [PHP Modules]
                Core
                json
                PDO

                [Zend Modules]
                Zend OPcache
                "#,
        );

        assert_eq!(
            extensions,
            vec![
                "core".to_string(),
                "json".to_string(),
                "pdo".to_string(),
                "zend opcache".to_string(),
            ]
        );
    }

    #[test]
    fn accepts_extension_requirement_case_insensitively() {
        let requirements = vec![required_package("Ext-JSON", "*")];
        let platform = platform("8.3.0", &["json"]);

        let result = validate(&requirements, &platform, "symfony/console");

        assert!(result.is_ok());
    }

    #[test]
    fn accepts_platform_requirement_name_case_insensitively() {
        let requirements = vec![required_package("PHP", ">=8.1")];
        let platform = platform("8.3.0", &[]);

        let result = validate(&requirements, &platform, "symfony/console");

        assert!(result.is_ok());
    }

    fn required_package(name: &str, constraint: &str) -> RequiredPackage {
        RequiredPackage {
            name: name.to_string(),
            constraint: constraint.to_string(),
        }
    }

    fn platform(php_version: &str, extensions: &[&str]) -> Platform {
        Platform {
            php_version: php_version.to_string(),
            extensions: extensions
                .iter()
                .map(|extension| extension.to_string())
                .collect(),
        }
    }
}
