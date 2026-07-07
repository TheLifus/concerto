use crate::composer::RequiredPackage;
use crate::error::{ConcertoError, Result};
use std::collections::HashMap;
use std::process::Command;

const PLATFORM_EXTENSIONS_ENV: &str = "CONCERTO_PLATFORM_EXTENSIONS";
const PLATFORM_PHP_ENV: &str = "CONCERTO_PLATFORM_PHP";
const PHP_PLATFORM_SCRIPT: &str = "echo PHP_VERSION, PHP_EOL; foreach \
    (get_loaded_extensions() as $extension) { echo $extension, '=', phpversion($extension) ?: '', PHP_EOL; }";

#[derive(Debug)]
pub(crate) struct Platform {
    pub php_version: String,
    pub extensions: Vec<String>,
    pub extension_versions: HashMap<String, String>,
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

    if !platform
        .extensions
        .iter()
        .any(|installed| installed == extension)
    {
        return Err(platform_error(package_name, requirement, "missing"));
    }

    if requirement.constraint == "*" {
        return Ok(());
    }

    let Some(version) = platform.extension_versions.get(extension) else {
        return Err(platform_error(
            package_name,
            requirement,
            "installed, version unknown",
        ));
    };

    let matches =
        semver_php::Semver::satisfies(version, &requirement.constraint).map_err(|error| {
            ConcertoError::platform_detection(format!(
                "Could not check {} requirement: {error}",
                requirement.name
            ))
        })?;

    if matches {
        return Ok(());
    }

    Err(platform_error(package_name, requirement, version))
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
        let (extensions, extension_versions) = env_extensions();

        return Ok(Platform {
            php_version,
            extensions,
            extension_versions,
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
    let (extensions, extension_versions) = lines
        .map(parse_extension_line)
        .filter(|(extension, _)| !extension.is_empty())
        .fold(
            (Vec::new(), HashMap::new()),
            |(mut extensions, mut versions), (extension, version)| {
                extensions.push(extension.clone());
                if !version.is_empty() {
                    versions.insert(extension, version);
                }
                (extensions, versions)
            },
        );

    Ok(Platform {
        php_version,
        extensions,
        extension_versions,
    })
}

fn env_extensions() -> (Vec<String>, HashMap<String, String>) {
    std::env::var(PLATFORM_EXTENSIONS_ENV)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(parse_extension_line)
        .fold(
            (Vec::new(), HashMap::new()),
            |(mut extensions, mut versions), (extension, version)| {
                extensions.push(extension.clone());
                if !version.is_empty() {
                    versions.insert(extension, version);
                }
                (extensions, versions)
            },
        )
}

fn parse_extension_line(line: &str) -> (String, String) {
    let (extension, version) = line
        .split_once('=')
        .or_else(|| line.split_once(':'))
        .unwrap_or((line, ""));

    (extension.trim().to_lowercase(), version.trim().to_string())
}

#[cfg(test)]
mod tests;
