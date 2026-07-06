use crate::composer::RequiredPackage;
use crate::error::{ConcertoError, Result};
use std::process::Command;

const PLATFORM_EXTENSIONS_ENV: &str = "CONCERTO_PLATFORM_EXTENSIONS";
const PLATFORM_PHP_ENV: &str = "CONCERTO_PLATFORM_PHP";
const PHP_PLATFORM_SCRIPT: &str = "echo PHP_VERSION, PHP_EOL; foreach \
    (get_loaded_extensions() as $extension) { echo $extension, PHP_EOL; }";

#[derive(Debug)]
pub(crate) struct Platform {
    pub php_version: String,
    pub extensions: Vec<String>,
}

pub(crate) fn validate(
    requirements: &[RequiredPackage],
    platform: &Platform,
    package_name: &str,
) -> Result<()> {
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
) -> Result<()> {
    let matches = semver_php::Semver::satisfies(&platform.php_version, &requirement.constraint)
        .map_err(|error| {
            ConcertoError::platform_detection(format!("Could not check php requirement: {error}"))
        })?;

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
) -> Result<()> {
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

fn platform_error(
    package_name: &str,
    requirement: &RequiredPackage,
    detected: &str,
) -> ConcertoError {
    ConcertoError::platform(
        package_name,
        format!("{} {}", requirement.name, requirement.constraint),
        detected,
    )
}

pub(crate) fn current() -> Result<Platform> {
    if let Ok(php_version) = std::env::var(PLATFORM_PHP_ENV) {
        return Ok(Platform {
            php_version,
            extensions: env_extensions(),
        });
    }

    let output = command_output("php", &["-r", PHP_PLATFORM_SCRIPT])?;

    parse_platform(&output)
}

fn command_output(command: &str, arguments: &[&str]) -> Result<String> {
    let output = Command::new(command)
        .args(arguments)
        .output()
        .map_err(|error| {
            ConcertoError::platform_detection(format!("Could not run {command}: {error}"))
        })?;

    if !output.status.success() {
        return Err(ConcertoError::platform_detection(format!(
            "{command} exited with {}",
            output.status
        )));
    }

    String::from_utf8(output.stdout).map_err(|error| {
        ConcertoError::platform_detection(format!("{command} output is not valid UTF-8: {error}"))
    })
}

fn parse_platform(output: &str) -> Result<Platform> {
    let mut lines = output.lines();
    let php_version = lines
        .next()
        .ok_or_else(|| ConcertoError::platform_detection("Could not detect PHP version"))?
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

fn env_extensions() -> Vec<String> {
    std::env::var(PLATFORM_EXTENSIONS_ENV)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests;
