use crate::composer::package_path_parts;
use crate::error::{ConcertoError, Result};
use crate::lockfile::{LockedPackage, Lockfile};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const AUTOLOAD_PATH: &str = "vendor/autoload.php";
const LOADER_PATH: &str = "vendor/concerto_autoload.php";

#[derive(Default)]
struct AutoloadMap {
    psr4: BTreeMap<String, Vec<String>>,
    psr0: BTreeMap<String, Vec<String>>,
    classmap: BTreeMap<String, String>,
    files: Vec<String>,
}

struct AutoloadBackup {
    path: PathBuf,
    backup: Option<PathBuf>,
}

pub(crate) fn write(lockfile: &Lockfile, root_composer_json: &str) -> Result<()> {
    let autoload = read_autoload_map(lockfile, root_composer_json)?;
    let files = [
        (Path::new(LOADER_PATH), loader_file(&autoload)?),
        (Path::new(AUTOLOAD_PATH), autoload_file()),
    ];

    write_autoload_files(&files)
}

fn write_autoload_files(files: &[(&Path, String)]) -> Result<()> {
    let mut temps = Vec::with_capacity(files.len());

    for (path, content) in files {
        let temp = temporary_autoload_path(path)?;

        if let Err(error) = std::fs::write(&temp, content) {
            remove_temporary_autoload_files(&temps);

            return Err(ConcertoError::autoload(format!(
                "Could not write temporary autoload file {}: {error}",
                temp.display()
            )));
        }

        temps.push(temp);
    }

    let backups = match backup_autoload_files(files) {
        Ok(backups) => backups,
        Err(error) => {
            remove_temporary_autoload_files(&temps);

            return Err(error);
        }
    };

    for ((path, _), temp) in files.iter().zip(&temps) {
        if let Err(error) = std::fs::rename(temp, path) {
            restore_autoload_files(&backups);
            remove_temporary_autoload_files(&temps);

            return Err(ConcertoError::autoload(format!(
                "Could not publish autoload file {}: {error}",
                path.display()
            )));
        }
    }

    remove_autoload_backups(&backups);

    Ok(())
}

fn temporary_autoload_path(path: &Path) -> Result<PathBuf> {
    sidecar_autoload_path(path, "tmp")
}

fn backup_autoload_path(path: &Path) -> Result<PathBuf> {
    sidecar_autoload_path(path, &format!("{}.bak", std::process::id()))
}

fn sidecar_autoload_path(path: &Path, suffix: &str) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ConcertoError::autoload(format!("Invalid autoload path {}", path.display()))
        })?;

    Ok(path.with_file_name(format!("{file_name}.{suffix}")))
}

fn backup_autoload_files(files: &[(&Path, String)]) -> Result<Vec<AutoloadBackup>> {
    let mut backups = Vec::with_capacity(files.len());

    for (path, _) in files {
        match std::fs::symlink_metadata(path) {
            Ok(_) => {
                let backup = backup_autoload_path(path)?;

                let _ = std::fs::remove_file(&backup);
                if let Err(error) = std::fs::rename(path, &backup) {
                    restore_autoload_files(&backups);

                    return Err(ConcertoError::autoload(format!(
                        "Could not backup autoload file {}: {error}",
                        path.display()
                    )));
                }

                backups.push(AutoloadBackup {
                    path: path.to_path_buf(),
                    backup: Some(backup),
                });
            }
            Err(error) if error.kind() == ErrorKind::NotFound => backups.push(AutoloadBackup {
                path: path.to_path_buf(),
                backup: None,
            }),
            Err(error) => {
                restore_autoload_files(&backups);

                return Err(ConcertoError::autoload(format!(
                    "Could not inspect autoload file {}: {error}",
                    path.display()
                )));
            }
        }
    }

    Ok(backups)
}

fn remove_temporary_autoload_files(paths: &[PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

fn restore_autoload_files(backups: &[AutoloadBackup]) {
    for backup in backups.iter().rev() {
        let _ = std::fs::remove_file(&backup.path);

        if let Some(backup_path) = &backup.backup {
            let _ = std::fs::rename(backup_path, &backup.path);
        }
    }
}

fn remove_autoload_backups(backups: &[AutoloadBackup]) {
    for backup in backups {
        if let Some(backup_path) = &backup.backup {
            let _ = std::fs::remove_file(backup_path);
        }
    }
}

fn read_autoload_map(lockfile: &Lockfile, root_composer_json: &str) -> Result<AutoloadMap> {
    let mut autoload = AutoloadMap::default();

    for package in package_file_order(lockfile) {
        let package_path = vendor_package_path(&package.name)?;
        collect_package_autoload(&mut autoload, &package_path)?;
    }

    collect_root_autoload(&mut autoload, root_composer_json)?;

    Ok(autoload)
}

fn package_file_order(lockfile: &Lockfile) -> Vec<&LockedPackage> {
    let packages = lockfile
        .packages
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::new();
    let mut ordered = Vec::new();

    for package in packages.values() {
        visit_package(package, &packages, &mut visited, &mut ordered);
    }

    ordered
}

fn visit_package<'a>(
    package: &'a LockedPackage,
    packages: &BTreeMap<&str, &'a LockedPackage>,
    visited: &mut BTreeSet<&'a str>,
    ordered: &mut Vec<&'a LockedPackage>,
) {
    if !visited.insert(package.name.as_str()) {
        return;
    }

    let mut requirements = package
        .package_requires
        .iter()
        .filter_map(|requirement| packages.get(requirement.name.as_str()))
        .copied()
        .collect::<Vec<_>>();

    requirements.sort_by(|left, right| left.name.cmp(&right.name));

    for requirement in requirements {
        visit_package(requirement, packages, visited, ordered);
    }

    ordered.push(package);
}

fn collect_package_autoload(autoload: &mut AutoloadMap, package_path: &Path) -> Result<()> {
    let composer_json = package_path.join("composer.json");

    if !composer_json.exists() {
        return Ok(());
    }

    let parsed = read_json_file(&composer_json)?;
    collect_autoload_sections(autoload, &parsed, package_path)
}

fn collect_root_autoload(autoload: &mut AutoloadMap, root_composer_json: &str) -> Result<()> {
    let parsed: Value = serde_json::from_str(root_composer_json)
        .map_err(|error| ConcertoError::autoload(format!("Invalid composer.json: {error}")))?;

    collect_autoload_sections(autoload, &parsed, Path::new("."))
}

fn collect_autoload_sections(
    autoload: &mut AutoloadMap,
    composer_json: &Value,
    package_path: &Path,
) -> Result<()> {
    let Some(autoload_json) = composer_json.get("autoload") else {
        return Ok(());
    };

    collect_namespace_map(autoload_json, "psr-4", package_path, &mut autoload.psr4)?;
    collect_namespace_map(autoload_json, "psr-0", package_path, &mut autoload.psr0)?;
    collect_files(autoload_json, package_path, &mut autoload.files)?;
    collect_classmap(autoload_json, package_path, &mut autoload.classmap)
}

fn collect_namespace_map(
    autoload_json: &Value,
    section: &str,
    package_path: &Path,
    mappings: &mut BTreeMap<String, Vec<String>>,
) -> Result<()> {
    let Some(map) = autoload_json
        .get(section)
        .and_then(|section| section.as_object())
    else {
        return Ok(());
    };

    for (namespace, paths) in map {
        for path in autoload_paths(paths, section)? {
            mappings
                .entry(namespace.to_string())
                .or_default()
                .push(autoload_path(package_path, path)?);
        }
    }

    Ok(())
}

fn collect_files(
    autoload_json: &Value,
    package_path: &Path,
    files: &mut Vec<String>,
) -> Result<()> {
    let Some(values) = autoload_json.get("files") else {
        return Ok(());
    };

    for path in autoload_paths(values, "files")? {
        files.push(autoload_path(package_path, path)?);
    }

    Ok(())
}

fn collect_classmap(
    autoload_json: &Value,
    package_path: &Path,
    classmap: &mut BTreeMap<String, String>,
) -> Result<()> {
    let Some(values) = autoload_json.get("classmap") else {
        return Ok(());
    };

    for path in autoload_paths(values, "classmap")? {
        for file in php_files(package_path.join(path))? {
            let content = std::fs::read_to_string(&file).map_err(|error| {
                ConcertoError::autoload(format!("Could not read classmap file: {error}"))
            })?;

            let file = absolute_path(file)?;

            for class in php_symbols(&content) {
                classmap.insert(class, file.clone());
            }
        }
    }

    Ok(())
}

fn autoload_paths<'a>(value: &'a Value, section: &str) -> Result<Vec<&'a str>> {
    if let Some(path) = value.as_str() {
        return Ok(vec![path]);
    }

    value
        .as_array()
        .ok_or_else(|| {
            ConcertoError::autoload(format!(
                "autoload.{section} must be a string or an array of strings"
            ))
        })?
        .iter()
        .map(|path| {
            path.as_str().ok_or_else(|| {
                ConcertoError::autoload(format!("autoload.{section} path must be a string"))
            })
        })
        .collect()
}

fn php_files(path: PathBuf) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(
            if path.extension().is_some_and(|extension| extension == "php") {
                vec![path]
            } else {
                Vec::new()
            },
        );
    }

    let mut files = Vec::new();

    if !path.exists() {
        return Ok(files);
    }

    collect_php_files(&path, &mut files)?;
    files.sort();

    Ok(files)
}

fn collect_php_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(path).map_err(|error| {
        ConcertoError::autoload(format!("Could not read classmap directory: {error}"))
    })? {
        let entry = entry.map_err(|error| {
            ConcertoError::autoload(format!("Could not read classmap entry: {error}"))
        })?;
        let path = entry.path();

        if path.is_dir() {
            collect_php_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "php") {
            files.push(path);
        }
    }

    Ok(())
}

fn php_symbols(content: &str) -> Vec<String> {
    let namespace = php_namespace(content);
    let words = content.split(|character: char| !is_php_symbol_character(character));
    let mut previous = "";
    let mut before_keyword = "";
    let mut pending_keyword = None;
    let mut symbols = Vec::new();

    for word in words {
        if word.is_empty() {
            continue;
        }

        if let Some(keyword) = pending_keyword.take() {
            if keyword != "class" || before_keyword != "new" {
                symbols.push(qualified_class_name(namespace.as_deref(), word));
            } else if is_php_type_keyword(word) {
                pending_keyword = Some(word);
                before_keyword = previous;
            }
        } else if is_php_type_keyword(word) {
            pending_keyword = Some(word);
            before_keyword = previous;
        }

        previous = word;
    }

    symbols
}

fn php_namespace(content: &str) -> Option<String> {
    let start = content.find("namespace ")? + "namespace ".len();
    let rest = &content[start..];
    let end = rest.find([';', '{'])?;
    let namespace = rest[..end].trim();

    if namespace.is_empty() {
        None
    } else {
        Some(namespace.to_string())
    }
}

fn is_php_type_keyword(word: &str) -> bool {
    matches!(word, "class" | "interface" | "trait" | "enum")
}

fn is_php_symbol_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_' || character == '\\'
}

fn qualified_class_name(namespace: Option<&str>, class: &str) -> String {
    match namespace {
        Some(namespace) => format!("{namespace}\\{class}"),
        None => class.to_string(),
    }
}

fn vendor_package_path(package_name: &str) -> Result<PathBuf> {
    let (vendor, package) = package_path_parts(package_name)?;

    Ok(PathBuf::from("vendor").join(vendor).join(package))
}

fn read_json_file(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path).map_err(|error| {
        ConcertoError::autoload(format!("Could not read {}: {error}", path.display()))
    })?;

    serde_json::from_str(&content)
        .map_err(|error| ConcertoError::autoload(format!("Invalid {}: {error}", path.display())))
}

fn autoload_file() -> String {
    r#"<?php

$loader = require __DIR__ . '/concerto_autoload.php';
spl_autoload_register($loader);
$loader('__concerto_files');

return $loader;
"#
    .to_string()
}

fn loader_file(autoload: &AutoloadMap) -> Result<String> {
    let data = AutoloadData::from(autoload);
    let data_json = serde_json::to_string(&data).map_err(|error| {
        ConcertoError::autoload(format!("Could not serialize autoload data: {error}"))
    })?;

    Ok(format!(
        r#"<?php

$autoloadJson = <<<'CONCERTO_AUTOLOAD_JSON'
{data_json}
CONCERTO_AUTOLOAD_JSON;

$autoload = json_decode($autoloadJson, true, 512, JSON_THROW_ON_ERROR);

return function (string $class) use ($autoload): void {{
{LOADER_BODY}"#
    ))
}

fn autoload_path(package_path: &Path, path: &str) -> Result<String> {
    absolute_path(package_path.join(path))
}

fn absolute_path(path: PathBuf) -> Result<String> {
    if path.is_absolute() {
        return Ok(display_path(&path));
    }

    let current_dir = std::env::current_dir().map_err(|error| {
        ConcertoError::autoload(format!("Could not read current directory: {error}"))
    })?;

    Ok(display_path(&current_dir.join(path)))
}

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[derive(serde::Serialize)]
struct AutoloadData<'a> {
    files: &'a [String],
    classmap: &'a BTreeMap<String, String>,
    psr4: &'a BTreeMap<String, Vec<String>>,
    psr0: &'a BTreeMap<String, Vec<String>>,
}

impl<'a> From<&'a AutoloadMap> for AutoloadData<'a> {
    fn from(autoload: &'a AutoloadMap) -> Self {
        Self {
            files: &autoload.files,
            classmap: &autoload.classmap,
            psr4: &autoload.psr4,
            psr0: &autoload.psr0,
        }
    }
}

const LOADER_BODY: &str = r#"    if ($class === '__concerto_files') {
        foreach ($autoload['files'] as $file) {
            require_once $file;
        }

        return;
    }

    if (isset($autoload['classmap'][$class]) && is_file($autoload['classmap'][$class])) {
        require $autoload['classmap'][$class];
        return;
    }

    foreach ($autoload['psr4'] as $prefix => $directories) {
        if (!str_starts_with($class, $prefix)) {
            continue;
        }

        $relativeClass = substr($class, strlen($prefix));
        $relativePath = str_replace('\\', '/', $relativeClass) . '.php';

        foreach ($directories as $directory) {
            $file = $directory . '/' . $relativePath;

            if (is_file($file)) {
                require $file;
                return;
            }
        }
    }

    foreach ($autoload['psr0'] as $prefix => $directories) {
        if ($prefix !== '' && !str_starts_with($class, $prefix)) {
            continue;
        }

        $relativePath = str_replace(['\\', '_'], '/', $class) . '.php';

        foreach ($directories as $directory) {
            $file = $directory . '/' . $relativePath;

            if (is_file($file)) {
                require $file;
                return;
            }
        }
    }
};
"#;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
