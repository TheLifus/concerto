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
    let output = command_output(
        "php",
        &[
            "-r",
            "echo PHP_VERSION, PHP_EOL; foreach (get_loaded_extensions() as $extension) { echo $extension, PHP_EOL; }",
        ],
    )?;

    parse_platform(&output)
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

fn parse_platform(output: &str) -> Result<Platform, String> {
    let mut lines = output.lines();
    let php_version = lines
        .next()
        .ok_or_else(|| "Could not detect PHP version".to_string())?
        .trim()
        .to_string();
    let extensions = lines
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_lowercase)
        .collect();

    Ok(Platform {
        php_version,
        extensions,
    })
}

#[cfg(test)]
mod tests;
