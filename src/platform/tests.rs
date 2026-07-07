use super::*;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

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

    let error = validate(&requirements, &platform, "symfony/console")
        .unwrap_err()
        .to_string();

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

    let error = validate(&requirements, &platform, "symfony/console")
        .unwrap_err()
        .to_string();

    assert!(error.contains("symfony/console"));
    assert!(error.contains("ext-intl"));
    assert!(error.contains("*"));
    assert!(error.contains("missing"));
}

#[test]
fn rejects_library_requirement_as_unsupported() {
    let requirements = vec![required_package("lib-icu", ">=70")];
    let platform = platform("8.3.0", &[]);

    let error = validate(&requirements, &platform, "symfony/intl")
        .unwrap_err()
        .to_string();

    assert!(error.contains("symfony/intl"));
    assert!(error.contains("lib-icu"));
    assert!(error.contains(">=70"));
    assert!(error.contains("unsupported"));
}

#[test]
fn parses_platform_from_php_output() {
    let platform = parse_platform(
        r#"
8.3.1
Core=8.3.1
json=8.3.1
PDO
"#
        .trim_start(),
    )
    .unwrap();

    assert_eq!(platform.php_version, "8.3.1");
    assert_eq!(
        platform.extensions,
        vec!["core".to_string(), "json".to_string(), "pdo".to_string()]
    );
    assert_eq!(platform.extension_versions["json"], "8.3.1");
}

#[test]
fn accepts_matching_extension_version_requirement() {
    let requirements = vec![required_package("ext-json", ">=1.7")];
    let mut platform = platform("8.3.0", &["json"]);
    platform
        .extension_versions
        .insert("json".to_string(), "1.7.0".to_string());

    let result = validate(&requirements, &platform, "symfony/console");

    assert!(result.is_ok());
}

#[test]
fn rejects_unmet_extension_version_requirement() {
    let requirements = vec![required_package("ext-json", ">=2.0")];
    let mut platform = platform("8.3.0", &["json"]);
    platform
        .extension_versions
        .insert("json".to_string(), "1.7.0".to_string());

    let error = validate(&requirements, &platform, "symfony/console")
        .unwrap_err()
        .to_string();

    assert!(error.contains("ext-json"));
    assert!(error.contains(">=2.0"));
    assert!(error.contains("1.7.0"));
}

#[test]
fn rejects_extension_version_requirement_when_version_is_unknown() {
    let requirements = vec![required_package("ext-json", ">=1.0")];
    let platform = platform("8.3.0", &["json"]);

    let error = validate(&requirements, &platform, "symfony/console")
        .unwrap_err()
        .to_string();

    assert!(error.contains("ext-json"));
    assert!(error.contains("version unknown"));
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

#[test]
fn detects_when_extension_metadata_is_needed() {
    assert!(!needs_extension_metadata(&[required_package(
        "php", ">=8.1"
    )]));
    assert!(needs_extension_metadata(&[required_package(
        "ext-json", "*"
    )]));
    assert!(needs_extension_metadata(&[required_package(
        "Ext-JSON", "*"
    )]));
}

#[test]
fn parses_php_version_only_output() {
    let platform = parse_platform("8.3.1\n").unwrap();

    assert_eq!(platform.php_version, "8.3.1");
    assert!(platform.extensions.is_empty());
    assert!(platform.extension_versions.is_empty());
}

#[test]
fn caches_php_and_full_platform_slots_independently() {
    let dir = temp_dir("platform-cache-slots");
    let cache_path = dir.join("platform-php.json");
    let key = cache_key("php-a");
    let php_platform = platform("8.3.1", &[]);
    let full_platform = platform("8.3.1", &["json"]);

    write_cached_platform_to(
        &cache_path,
        PlatformCacheSlot::Php,
        key.clone(),
        &php_platform,
    )
    .unwrap();
    write_cached_platform_to(
        &cache_path,
        PlatformCacheSlot::Full,
        key.clone(),
        &full_platform,
    )
    .unwrap();

    assert_eq!(
        read_cached_platform_from(&cache_path, PlatformCacheSlot::Php, &key).unwrap(),
        Some(php_platform)
    );
    assert_eq!(
        read_cached_platform_from(&cache_path, PlatformCacheSlot::Full, &key).unwrap(),
        Some(full_platform)
    );

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ignores_platform_cache_when_key_differs() {
    let dir = temp_dir("platform-cache-miss");
    let cache_path = dir.join("platform-php.json");

    write_cached_platform_to(
        &cache_path,
        PlatformCacheSlot::Php,
        cache_key("php-a"),
        &platform("8.3.1", &[]),
    )
    .unwrap();

    assert_eq!(
        read_cached_platform_from(&cache_path, PlatformCacheSlot::Php, &cache_key("php-b"))
            .unwrap(),
        None
    );

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn ignores_invalid_platform_cache() {
    let dir = temp_dir("platform-cache-invalid");
    let cache_path = dir.join("platform-php.json");

    std::fs::write(&cache_path, "not json").unwrap();

    assert_eq!(
        read_cached_platform_from(&cache_path, PlatformCacheSlot::Php, &cache_key("php-a"))
            .unwrap(),
        None
    );

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn builds_platform_cache_key_from_file_metadata() {
    let dir = temp_dir("platform-cache-key");
    let php = dir.join("php");

    std::fs::write(&php, "fake php").unwrap();

    let key = platform_cache_key(&php).unwrap();

    assert!(key.path.ends_with("php"));
    assert_eq!(key.len, "fake php".len() as u64);

    std::fs::remove_dir_all(dir).unwrap();
}

fn required_package(name: &str, constraint: &str) -> RequiredPackage {
    RequiredPackage {
        name: name.to_string(),
        constraint: constraint.to_string(),
    }
}

fn cache_key(path: &str) -> PlatformCacheKey {
    PlatformCacheKey {
        path: path.to_string(),
        len: 1,
        modified_secs: 2,
        modified_nanos: 3,
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("concerto-{name}-{nanos}"));

    std::fs::create_dir_all(&path).unwrap();

    path
}

fn platform(php_version: &str, extensions: &[&str]) -> Platform {
    Platform {
        php_version: php_version.to_string(),
        extensions: extensions
            .iter()
            .map(|extension| extension.to_string())
            .collect(),
        extension_versions: HashMap::new(),
    }
}
