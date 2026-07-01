use crate::lockfile::{self, LockedPackage, Lockfile};
use std::collections::HashMap;
use std::path::Path;

use crate::composer::{RequiredPackage, required_packages};
use crate::http::get_text;
use crate::package_store::{self, PackageArchive};
use crate::packagist::{self, PackagistRelease};
use crate::perf::PerfLogger;
use std::time::Instant;

pub(crate) const NO_COMPOSER_JSON: &str = "No composer.json found";

struct ResolvedPackageEntry {
    version: String,
    dist_url: String,
    constraints: Vec<String>,
    package_requires: Vec<RequiredPackage>,
    platform_requires: Vec<RequiredPackage>,
}

struct ResolvedPackage {
    release: PackagistRelease,
    metadata_url: String,
}

type PackageConstraints = HashMap<String, Vec<String>>;
type ResolvedPackages = HashMap<String, ResolvedPackageEntry>;

pub fn install() -> Result<(), String> {
    let perf = PerfLogger::from_env()?;
    let install_started_at = Instant::now();
    let content =
        std::fs::read_to_string("composer.json").map_err(|_| NO_COMPOSER_JSON.to_string())?;

    let packages = required_packages(&content)?;

    if let Some(lockfile) = lockfile::read()? {
        if lockfile::matches_root_requirements(&lockfile, &packages) {
            println!(
                "Installing from lockfile with {} packages",
                lockfile.packages.len()
            );

            let lockfile_started_at = Instant::now();

            for package in &lockfile.packages {
                install_locked_package(package, &perf)?;
            }

            perf.log(
                "lockfile_install",
                lockfile_started_at.elapsed(),
                &[("packages", lockfile.packages.len().to_string())],
            )?;
            perf.finish_run(install_started_at.elapsed(), lockfile.packages.len())?;

            return Ok(());
        }

        println!("Ignoring outdated lockfile");
    }

    std::fs::create_dir_all(".concerto/store")
        .map_err(|error| format!("Could not create local store: {error}"))?;

    std::fs::create_dir_all("vendor")
        .map_err(|error| format!("Could not create vendor directory: {error}"))?;

    let mut package_constraints = PackageConstraints::new();

    for package in &packages {
        add_package_constraint(&mut package_constraints, package);
    }

    let mut resolved_packages = ResolvedPackages::new();

    for package in &packages {
        install_package(
            package,
            &mut package_constraints,
            &mut resolved_packages,
            &perf,
        )?;
    }

    let package_count = resolved_packages.len();
    let lockfile = build_lockfile(packages, resolved_packages);
    let lockfile_started_at = Instant::now();
    lockfile::write(&lockfile)?;
    perf.log("lockfile_write", lockfile_started_at.elapsed(), &[])?;
    perf.finish_run(install_started_at.elapsed(), package_count)?;

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
    perf: &PerfLogger,
) -> Result<ResolvedPackage, String> {
    let started_at = Instant::now();
    let metadata_url = packagist::package_url(&package.name)?;
    let metadata = get_text(&metadata_url)?;
    println!("Fetched {} bytes", metadata.len());

    let release = packagist::first_release_candidate(&metadata, &package.name, constraints)?;
    perf.log(
        "resolve_package",
        started_at.elapsed(),
        &[
            ("package", package.name.clone()),
            ("version", release.version.clone()),
        ],
    )?;

    Ok(ResolvedPackage {
        release,
        metadata_url,
    })
}

fn install_package(
    package: &RequiredPackage,
    package_constraints: &mut PackageConstraints,
    resolved_packages: &mut ResolvedPackages,
    perf: &PerfLogger,
) -> Result<(), String> {
    if let Some(resolved_package) = resolved_packages.get(&package.name) {
        return ensure_resolved_package_matches(package, resolved_package);
    }

    let constraints = package_constraints
        .get(&package.name)
        .ok_or_else(|| format!("Missing constraints for {}", package.name))?;
    let resolved = resolve_package(package, constraints, perf)?;

    resolved_packages.insert(
        package.name.clone(),
        ResolvedPackageEntry {
            version: resolved.release.version.clone(),
            dist_url: resolved.release.dist_url.clone(),
            constraints: constraints.clone(),
            package_requires: resolved.release.package_requires.clone(),
            platform_requires: resolved.release.platform_requires.clone(),
        },
    );

    install_resolved_package(package, &resolved, perf)?;

    for requirement in &resolved.release.package_requires {
        add_package_constraint(package_constraints, requirement);
        install_package(requirement, package_constraints, resolved_packages, perf)?;
    }

    Ok(())
}

fn ensure_resolved_package_matches(
    package: &RequiredPackage,
    resolved_package: &ResolvedPackageEntry,
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
    perf: &PerfLogger,
) -> Result<(), String> {
    print_release(&resolved.release);
    print_requirements(&resolved.release);

    let source_started_at = Instant::now();
    let archive = PackageArchive {
        version: &resolved.release.version,
        dist_url: &resolved.release.dist_url,
    };

    let source = package_store::prepare_source(&package.name, archive)?;
    let source_event = if source.is_reused() {
        println!("Reusing {}", source.path().display());
        "source_reuse"
    } else {
        "source_download_extract"
    };
    perf.log(
        source_event,
        source_started_at.elapsed(),
        &[("package", package.name.clone())],
    )?;

    let link_started_at = Instant::now();
    package_store::link_to_vendor(&source)?;
    perf.log(
        "vendor_link",
        link_started_at.elapsed(),
        &[("package", package.name.clone())],
    )?;
    print_install_summary(package, source.path(), &resolved.metadata_url);

    Ok(())
}

fn install_locked_package(package: &LockedPackage, perf: &PerfLogger) -> Result<(), String> {
    let archive = PackageArchive {
        version: &package.version,
        dist_url: &package.dist_url,
    };

    let source_started_at = Instant::now();
    let source = package_store::prepare_source(&package.name, archive)?;
    let source_event = if source.is_reused() {
        println!("Reusing {}", source.path().display());
        "source_reuse"
    } else {
        "source_download_extract"
    };

    perf.log(
        source_event,
        source_started_at.elapsed(),
        &[("package", package.name.clone())],
    )?;

    let link_started_at = Instant::now();
    package_store::link_to_vendor(&source)?;
    perf.log(
        "vendor_link",
        link_started_at.elapsed(),
        &[("package", package.name.clone())],
    )?;

    println!(
        "{} {} -> {}",
        package.name,
        package.version,
        source.path().display()
    );

    Ok(())
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

fn build_lockfile(
    root_requirements: Vec<RequiredPackage>,
    resolved_packages: ResolvedPackages,
) -> Lockfile {
    let mut packages = resolved_packages
        .into_iter()
        .map(|(name, package)| LockedPackage {
            name,
            version: package.version,
            dist_url: package.dist_url,
            package_requires: package.package_requires,
            platform_requires: package.platform_requires,
        })
        .collect::<Vec<_>>();

    packages.sort_by(|left, right| left.name.cmp(&right.name));

    Lockfile {
        root_requirements,
        packages,
    }
}
