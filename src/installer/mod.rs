mod lockfile_install;
mod resolved_install;

use crate::autoload;
use crate::lockfile::{self, LockedPackage, Lockfile};
use std::path::Path;

use crate::composer::{RequiredPackage, required_packages};
use crate::package_store::{self, PackageArchive};
use crate::perf::PerfLogger;
use crate::platform;
use crate::resolver::{self, ResolvedPackages};
use std::time::{Duration, Instant};

pub(crate) const NO_COMPOSER_JSON: &str = "No composer.json found";

pub(super) struct PackageSourcePreparation {
    source: package_store::PackageSource,
    duration: Duration,
    event: &'static str,
}

pub fn install() -> Result<(), String> {
    let perf = PerfLogger::from_env()?;
    let install_started_at = Instant::now();
    let content =
        std::fs::read_to_string("composer.json").map_err(|_| NO_COMPOSER_JSON.to_string())?;

    let packages = required_packages(&content)?;
    let platform_started_at = Instant::now();
    let platform = platform::current()?;
    perf.log("platform_current", platform_started_at.elapsed(), &[])?;

    if let Some(lockfile) = lockfile::read()? {
        if lockfile::matches_root_requirements(&lockfile, &packages) {
            println!(
                "Installing from lockfile with {} packages",
                lockfile.packages.len()
            );
            validate_locked_platform_requirements(&lockfile.packages, &platform)?;
            let lockfile_started_at = Instant::now();

            lockfile_install::install(&lockfile.packages, &perf)?;

            write_autoload(&lockfile, &content, &perf)?;
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
    validate_resolved_platform_requirements(&resolved_packages, &platform)?;
    let package_count = resolved_packages.len();
    resolved_install::install(&resolved_packages, &perf)?;

    let lockfile = build_lockfile(packages, resolved_packages);
    write_autoload(&lockfile, &content, &perf)?;
    let lockfile_started_at = Instant::now();
    lockfile::write(&lockfile)?;
    perf.log("lockfile_write", lockfile_started_at.elapsed(), &[])?;
    perf.finish_run(install_started_at.elapsed(), package_count)?;

    Ok(())
}

fn write_autoload(
    lockfile: &Lockfile,
    root_composer_json: &str,
    perf: &PerfLogger,
) -> Result<(), String> {
    let started_at = Instant::now();

    autoload::write(lockfile, root_composer_json)?;
    perf.log(
        "autoload_write",
        started_at.elapsed(),
        &[("packages", lockfile.packages.len().to_string())],
    )
}

fn validate_locked_platform_requirements(
    packages: &[LockedPackage],
    platform: &platform::Platform,
) -> Result<(), String> {
    for package in packages {
        platform::validate(&package.platform_requires, platform, &package.name)?;
    }

    Ok(())
}

fn validate_resolved_platform_requirements(
    packages: &ResolvedPackages,
    platform: &platform::Platform,
) -> Result<(), String> {
    for (name, package) in packages {
        platform::validate(&package.platform_requires, platform, name)?;
    }

    Ok(())
}
fn prepare_package_source(
    name: &str,
    version: &str,
    dist_url: &str,
) -> Result<PackageSourcePreparation, String> {
    let archive = PackageArchive { version, dist_url };
    let started_at = Instant::now();
    let source = package_store::prepare_source(name, archive)?;
    let source_event = if source.is_reused() {
        println!("Reusing {}", source.path().display());
        "source_reuse"
    } else {
        "source_download_extract"
    };

    Ok(PackageSourcePreparation {
        source,
        duration: started_at.elapsed(),
        event: source_event,
    })
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
    let mut root_requirements = root_requirements;
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

    root_requirements.sort_by(|left, right| left.name.cmp(&right.name));
    packages.sort_by(|left, right| left.name.cmp(&right.name));

    Lockfile {
        lockfile_version: lockfile::LOCKFILE_VERSION,
        root_requirements_hash: lockfile::root_requirements_hash(&root_requirements),
        root_requirements,
        packages,
    }
}

#[cfg(test)]
mod tests;
