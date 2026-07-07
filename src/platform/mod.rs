use crate::composer::RequiredPackage;
use crate::error::{ConcertoError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

const PLATFORM_CACHE_PATH: &str = ".concerto/cache/platform-php.json";
const PLATFORM_EXTENSIONS_ENV: &str = "CONCERTO_PLATFORM_EXTENSIONS";
const PLATFORM_PHP_ENV: &str = "CONCERTO_PLATFORM_PHP";
const PHP_PLATFORM_SCRIPT: &str = "echo PHP_VERSION, PHP_EOL; foreach \
    (get_loaded_extensions() as $extension) { echo $extension, '=', phpversion($extension) ?: '', PHP_EOL; }";
const PHP_VERSION_SCRIPT: &str = "echo PHP_VERSION, PHP_EOL;";

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub(crate) struct Platform {
    pub php_version: String,
    pub extensions: Vec<String>,
    pub extension_versions: HashMap<String, String>,
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
struct PlatformCache {
    php: Option<PlatformCacheEntry>,
    full: Option<PlatformCacheEntry>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct PlatformCacheEntry {
    key: PlatformCacheKey,
    platform: Platform,
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct PlatformCacheKey {
    path: String,
    len: u64,
    modified_secs: u64,
    modified_nanos: u32,
}

#[derive(Clone, Copy)]
enum PlatformCacheSlot {
    Php,
    Full,
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

    current_cached(PlatformCacheSlot::Full, PHP_PLATFORM_SCRIPT)
}

pub(crate) fn current_for(requirements: &[RequiredPackage]) -> Result<Platform> {
    if needs_extension_metadata(requirements) {
        return current();
    }

    if let Ok(php_version) = std::env::var(PLATFORM_PHP_ENV) {
        return Ok(Platform {
            php_version,
            extensions: Vec::new(),
            extension_versions: HashMap::new(),
        });
    }

    current_cached(PlatformCacheSlot::Php, PHP_VERSION_SCRIPT)
}

pub(crate) fn needs_extension_metadata(requirements: &[RequiredPackage]) -> bool {
    requirements
        .iter()
        .any(|requirement| requirement.name.to_lowercase().starts_with("ext-"))
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

fn current_cached(slot: PlatformCacheSlot, script: &str) -> Result<Platform> {
    let key = php_cache_key().ok();
    if let Some(key) = &key
        && let Some(platform) = read_cached_platform(slot, key)?
    {
        return Ok(platform);
    }

    let platform = parse_platform(&command_output("php", &["-r", script])?)?;

    if let Some(key) = key {
        let _ = write_cached_platform(slot, key, &platform);
    }

    Ok(platform)
}

fn read_cached_platform(
    slot: PlatformCacheSlot,
    key: &PlatformCacheKey,
) -> Result<Option<Platform>> {
    read_cached_platform_from(Path::new(PLATFORM_CACHE_PATH), slot, key)
}

fn read_cached_platform_from(
    path: &Path,
    slot: PlatformCacheSlot,
    key: &PlatformCacheKey,
) -> Result<Option<Platform>> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let Ok(cache) = serde_json::from_str::<PlatformCache>(&content) else {
        return Ok(None);
    };
    let entry = match slot {
        PlatformCacheSlot::Php => cache.php,
        PlatformCacheSlot::Full => cache.full,
    };

    Ok(entry
        .filter(|entry| entry.key == *key)
        .map(|entry| entry.platform))
}

fn write_cached_platform(
    slot: PlatformCacheSlot,
    key: PlatformCacheKey,
    platform: &Platform,
) -> Result<()> {
    write_cached_platform_to(Path::new(PLATFORM_CACHE_PATH), slot, key, platform)
}

fn write_cached_platform_to(
    path: &Path,
    slot: PlatformCacheSlot,
    key: PlatformCacheKey,
    platform: &Platform,
) -> Result<()> {
    let mut cache: PlatformCache = std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default();
    let entry = Some(PlatformCacheEntry {
        key,
        platform: platform.clone(),
    });

    match slot {
        PlatformCacheSlot::Php => cache.php = entry,
        PlatformCacheSlot::Full => cache.full = entry,
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ConcertoError::platform_detection(format!(
                "Could not create platform cache directory: {error}"
            ))
        })?;
    }

    let content = serde_json::to_string_pretty(&cache).map_err(|error| {
        ConcertoError::platform_detection(format!("Could not serialize platform cache: {error}"))
    })?;

    std::fs::write(path, content).map_err(|error| {
        ConcertoError::platform_detection(format!("Could not write platform cache: {error}"))
    })
}

fn php_cache_key() -> Result<PlatformCacheKey> {
    let path = find_command_path("php").ok_or_else(|| {
        ConcertoError::platform_detection("Could not find php in PATH for platform cache")
    })?;
    platform_cache_key(&path)
}

fn platform_cache_key(path: &Path) -> Result<PlatformCacheKey> {
    let canonical_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let metadata = std::fs::metadata(path).map_err(|error| {
        ConcertoError::platform_detection(format!("Could not read php metadata: {error}"))
    })?;
    let modified = metadata
        .modified()
        .and_then(|modified| {
            modified
                .duration_since(UNIX_EPOCH)
                .map_err(std::io::Error::other)
        })
        .map_err(|error| {
            ConcertoError::platform_detection(format!("Could not read php mtime: {error}"))
        })?;

    Ok(PlatformCacheKey {
        path: canonical_path.display().to_string(),
        len: metadata.len(),
        modified_secs: modified.as_secs(),
        modified_nanos: modified.subsec_nanos(),
    })
}

fn find_command_path(command: &str) -> Option<PathBuf> {
    if command.contains(std::path::MAIN_SEPARATOR) {
        let path = PathBuf::from(command);
        return path.is_file().then_some(path);
    }

    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|path| path.join(command))
        .find(|path| path.is_file())
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
