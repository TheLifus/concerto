use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::composer::{RequiredPackage, package_path_parts, required_packages};
use crate::http::{download_bytes, get_text};
use crate::packagist::{self, PackagistRelease};

pub(crate) const NO_COMPOSER_JSON: &str = "No composer.json found";

struct PackagePaths {
    vendor_link: PathBuf,
    zip: PathBuf,
    extract: PathBuf,
}
struct ResolvedPackageVersion {
    version: String,
    constraints: Vec<String>,
}

struct ResolvedPackage {
    release: PackagistRelease,
    metadata_url: String,
}

type PackageConstraints = HashMap<String, Vec<String>>;
type ResolvedPackages = HashMap<String, ResolvedPackageVersion>;

pub fn install() -> Result<(), String> {
    let content =
        std::fs::read_to_string("composer.json").map_err(|_| NO_COMPOSER_JSON.to_string())?;

    let packages = required_packages(&content)?;

    std::fs::create_dir_all(".concerto/store")
        .map_err(|error| format!("Could not create local store: {error}"))?;

    std::fs::create_dir_all("vendor")
        .map_err(|error| format!("Could not create vendor directory: {error}"))?;

    let mut package_constraints = PackageConstraints::new();

    for package in &packages {
        add_package_constraint(&mut package_constraints, package);
    }

    let mut resolved_packages = ResolvedPackages::new();

    for package in packages {
        install_package(&package, &mut package_constraints, &mut resolved_packages)?;
    }

    Ok(())
}

fn add_package_constraint(package_constraints: &mut PackageConstraints, package: &RequiredPackage) {
    package_constraints
        .entry(package.name.clone())
        .or_default()
        .push(package.constraint.clone());
}

fn resolve_package(
    package: &RequiredPackage,
    constraints: &[String],
) -> Result<ResolvedPackage, String> {
    let metadata_url = packagist::package_url(&package.name)?;
    let metadata = get_text(&metadata_url)?;
    println!("Fetched {} bytes", metadata.len());

    let release = packagist::first_release_candidate(&metadata, &package.name, constraints)?;

    Ok(ResolvedPackage {
        release,
        metadata_url,
    })
}

fn only_child_dir(path: &Path) -> Result<PathBuf, String> {
    let dirs = std::fs::read_dir(path)
        .map_err(|error| format!("Could not read extracted package directory: {error}"))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

    match dirs.as_slice() {
        [dir] => Ok(dir.clone()),
        _ => Err(format!(
            "Expected exactly one extracted directory in {}",
            path.display()
        )),
    }
}

fn install_package(
    package: &RequiredPackage,
    package_constraints: &mut PackageConstraints,
    resolved_packages: &mut ResolvedPackages,
) -> Result<(), String> {
    if let Some(resolved_package) = resolved_packages.get(&package.name) {
        return ensure_resolved_package_matches(package, resolved_package);
    }

    let constraints = package_constraints
        .get(&package.name)
        .ok_or_else(|| format!("Missing constraints for {}", package.name))?;
    let resolved = resolve_package(package, constraints)?;

    resolved_packages.insert(
        package.name.clone(),
        ResolvedPackageVersion {
            version: resolved.release.version.clone(),
            constraints: constraints.clone(),
        },
    );

    install_resolved_package(package, &resolved)?;

    for requirement in &resolved.release.package_requires {
        add_package_constraint(package_constraints, requirement);
        install_package(requirement, package_constraints, resolved_packages)?;
    }

    Ok(())
}

fn ensure_resolved_package_matches(
    package: &RequiredPackage,
    resolved_package: &ResolvedPackageVersion,
) -> Result<(), String> {
    let satisfies =
        semver_php::Semver::satisfies(&resolved_package.version, &package.constraint)
            .map_err(|error| format!("Could not check installed package constraint: {error}"))?;

    if satisfies {
        println!(
            "Skipping already installed {} {}",
            package.name, resolved_package.version
        );

        return Ok(());
    }

    Err(format!(
        "Version conflict for {}: resolved {} from {}, but requested {}",
        package.name,
        resolved_package.version,
        resolved_package.constraints.join(", "),
        package.constraint
    ))
}

fn install_resolved_package(
    package: &RequiredPackage,
    resolved: &ResolvedPackage,
) -> Result<(), String> {
    let paths = package_paths(package)?;

    print_release(&resolved.release);
    print_requirements(&resolved.release);

    let source = download_and_extract(&resolved.release, &paths)?;
    link_vendor_package(&paths.vendor_link, &source)?;
    print_install_summary(package, &source, &resolved.metadata_url);

    Ok(())
}

fn package_paths(package: &RequiredPackage) -> Result<PackagePaths, String> {
    let (vendor, name) = package_path_parts(&package.name)?;
    let store = PathBuf::from(".concerto/store").join(vendor).join(name);
    let vendor_parent = PathBuf::from("vendor").join(vendor);
    let vendor_link = vendor_parent.join(name);

    std::fs::create_dir_all(&store)
        .map_err(|error| format!("Could not create package store directory: {error}"))?;
    std::fs::create_dir_all(&vendor_parent)
        .map_err(|error| format!("Could not create vendor directory: {error}"))?;

    let store = std::fs::canonicalize(&store)
        .map_err(|error| format!("Could not resolve package store directory: {error}"))?;

    Ok(PackagePaths {
        zip: store.join("package.zip"),
        extract: store.join("source"),
        vendor_link,
    })
}

fn download_and_extract(
    release: &PackagistRelease,
    paths: &PackagePaths,
) -> Result<PathBuf, String> {
    let zip = download_bytes(&release.dist_url)?;

    std::fs::write(&paths.zip, zip)
        .map_err(|error| format!("Could not write package zip: {error}"))?;
    println!("Downloaded {}", paths.zip.display());

    if paths.extract.exists() {
        std::fs::remove_dir_all(&paths.extract)
            .map_err(|error| format!("Could not clean package source directory: {error}"))?;
    }

    safe_unzip::extract_file(&paths.extract, &paths.zip)
        .map_err(|error| format!("Could not extract package zip: {error}"))?;
    println!("Extracted {}", paths.extract.display());

    let source = only_child_dir(&paths.extract)?;
    println!("Source {}", source.display());

    Ok(source)
}

fn link_vendor_package(vendor_link: &Path, source: &Path) -> Result<(), String> {
    if let Ok(metadata) = std::fs::symlink_metadata(vendor_link) {
        if !metadata.file_type().is_symlink() {
            return Err(format!(
                "Vendor path already exists and is not a symlink: {}",
                vendor_link.display()
            ));
        }

        std::fs::remove_file(vendor_link)
            .map_err(|error| format!("Could not remove existing vendor link: {error}"))?;
    }

    std::os::unix::fs::symlink(source, vendor_link)
        .map_err(|error| format!("Could not link vendor package to source: {error}"))
}

fn print_release(release: &PackagistRelease) {
    println!("Found {} versions", release.version_count);
    println!("Selected {}", release.version);
    println!("Requires {} packages", release.package_requires.len());
    println!(
        "Requires {} platform packages",
        release.platform_requires.len()
    );
    println!("Dist {}", release.dist_url);
}

fn print_requirements(release: &PackagistRelease) {
    for requirement in &release.package_requires {
        println!(
            "Package requirement: {} {}",
            requirement.name, requirement.constraint
        );
    }

    for requirement in &release.platform_requires {
        println!(
            "Platform requirement: {} {}",
            requirement.name, requirement.constraint
        );
    }
}

fn print_install_summary(package: &RequiredPackage, source: &Path, metadata_url: &str) {
    println!(
        "{} {} -> {} ({})",
        package.name,
        package.constraint,
        source.display(),
        metadata_url
    );
}
