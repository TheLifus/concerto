use crate::lockfile::{self, LockedPackage, Lockfile};
use std::path::Path;

use crate::composer::{RequiredPackage, required_packages};
use crate::package_store::{self, PackageArchive};
use crate::perf::PerfLogger;
use crate::resolver::{self, ResolvedPackageEntry, ResolvedPackages};
use std::time::{Duration, Instant};

pub(crate) const NO_COMPOSER_JSON: &str = "No composer.json found";

struct PreparedPackage {
    name: String,
    constraint: String,
    metadata_url: String,
    source: package_store::PackageSource,
    source_duration: Duration,
    source_event: &'static str,
}

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

    let resolved_packages = resolver::resolve(&packages, &perf)?;
    let package_count = resolved_packages.len();
    install_resolved_packages(&resolved_packages, &perf)?;

    let lockfile = build_lockfile(packages, resolved_packages);
    let lockfile_started_at = Instant::now();
    lockfile::write(&lockfile)?;
    perf.log("lockfile_write", lockfile_started_at.elapsed(), &[])?;
    perf.finish_run(install_started_at.elapsed(), package_count)?;

    Ok(())
}

fn install_resolved_packages(
    resolved_packages: &ResolvedPackages,
    perf: &PerfLogger,
) -> Result<(), String> {
    let prepare_started_at = Instant::now();
    let prepared_packages = prepare_resolved_sources(resolved_packages)?;

    perf.log(
        "sources_prepare",
        prepare_started_at.elapsed(),
        &[("packages", prepared_packages.len().to_string())],
    )?;

    for package in prepared_packages {
        install_prepared_package(package, perf)?;
    }

    Ok(())
}

fn install_prepared_package(package: PreparedPackage, perf: &PerfLogger) -> Result<(), String> {
    perf.log(
        package.source_event,
        package.source_duration,
        &[("package", package.name.clone())],
    )?;

    let link_started_at = Instant::now();
    package_store::link_to_vendor(&package.source)?;
    perf.log(
        "vendor_link",
        link_started_at.elapsed(),
        &[("package", package.name.clone())],
    )?;
    print_install_summary(
        &package.name,
        &package.constraint,
        package.source.path(),
        &package.metadata_url,
    );

    Ok(())
}

fn prepare_resolved_sources(
    resolved_packages: &ResolvedPackages,
) -> Result<Vec<PreparedPackage>, String> {
    let mut packages = resolved_packages.iter().collect::<Vec<_>>();

    packages.sort_by(|left, right| left.0.cmp(right.0));

    std::thread::scope(|scope| {
        let handles = packages
            .into_iter()
            .map(|(name, package)| scope.spawn(move || prepare_resolved_source(name, package)))
            .collect::<Vec<_>>();

        let mut prepared = Vec::with_capacity(handles.len());

        for handle in handles {
            let package = handle
                .join()
                .map_err(|_| "Package source worker panicked".to_string())??;
            prepared.push(package);
        }

        Ok(prepared)
    })
}

fn prepare_resolved_source(
    name: &str,
    package: &ResolvedPackageEntry,
) -> Result<PreparedPackage, String> {
    let archive = PackageArchive {
        version: &package.version,
        dist_url: &package.dist_url,
    };
    let started_at = Instant::now();
    let source = package_store::prepare_source(name, archive)?;
    let source_event = if source.is_reused() {
        println!("Reusing {}", source.path().display());
        "source_reuse"
    } else {
        "source_download_extract"
    };

    Ok(PreparedPackage {
        name: name.to_string(),
        constraint: package.constraints.join(", "),
        metadata_url: package.metadata_url.clone(),
        source,
        source_duration: started_at.elapsed(),
        source_event,
    })
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

fn print_install_summary(
    package_name: &str,
    package_constraint: &str,
    source: &Path,
    metadata_url: &str,
) {
    println!(
        "{} {} -> {} ({})",
        package_name,
        package_constraint,
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
        lockfile_version: lockfile::LOCKFILE_VERSION,
        root_requirements_hash: lockfile::root_requirements_hash(&root_requirements),
        root_requirements,
        packages,
    }
}
