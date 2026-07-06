mod lockfile_install;
mod resolved_install;

use crate::autoload;
use crate::error::{ConcertoError, Result, StoreStep};
use crate::install_event::{InstallEventKind, InstallReporter, InstallSummary};
use crate::lockfile::{self, LockedPackage, Lockfile};

use crate::composer::{RequiredPackage, required_packages};
use crate::package_store::{self, PackageArchive};
use crate::perf::PerfLogger;
use crate::platform;
use crate::resolver::{self, ResolvedPackages};
use std::io::ErrorKind;
use std::time::{Duration, Instant};

pub(super) const MAX_PARALLEL_WORKERS: usize = 16;

pub(super) struct PackageSourcePreparation {
    source: package_store::PackageSource,
    duration: Duration,
    event: &'static str,
}

struct InstallContext<'a> {
    root_composer_json: &'a str,
    platform: &'a platform::Platform,
    perf: &'a PerfLogger,
    reporter: &'a InstallReporter,
    started_at: Instant,
}

pub fn install(reporter: InstallReporter) -> Result<InstallSummary> {
    reporter.emit(InstallEventKind::Started);

    let perf = PerfLogger::from_env()?;
    let install_started_at = Instant::now();
    let content = std::fs::read_to_string("composer.json").map_err(|error| {
        if error.kind() == ErrorKind::NotFound {
            ConcertoError::MissingComposerJson
        } else {
            ConcertoError::composer_json(format!("Could not read composer.json: {error}"))
        }
    })?;

    let packages = required_packages(&content)?;
    let platform_started_at = Instant::now();
    let platform = platform::current()?;
    reporter.emit(InstallEventKind::PlatformDetected {
        php_version: platform.php_version.clone(),
        extension_count: platform.extensions.len(),
    });
    perf.log("platform_current", platform_started_at.elapsed(), &[])?;
    let context = InstallContext {
        root_composer_json: &content,
        platform: &platform,
        perf: &perf,
        reporter: &reporter,
        started_at: install_started_at,
    };

    if let Some(lockfile) = lockfile::read()? {
        if lockfile::matches_root_requirements(&lockfile, &packages) {
            return install_from_lockfile(lockfile, &context);
        }

        reporter.emit(InstallEventKind::LockfileOutdated);
    }

    install_from_resolution(packages, &context)
}

fn install_from_lockfile(
    lockfile: Lockfile,
    context: &InstallContext<'_>,
) -> Result<InstallSummary> {
    context.reporter.emit(InstallEventKind::LockfileMatched {
        packages: lockfile.packages.len(),
    });
    validate_locked_platform_requirements(&lockfile.packages, context.platform)?;
    let lockfile_started_at = Instant::now();

    lockfile_install::install(&lockfile.packages, context.perf, context.reporter)?;

    write_autoload(&lockfile, context.root_composer_json, context.perf)?;
    context.reporter.emit(InstallEventKind::AutoloadWritten {
        packages: lockfile.packages.len(),
    });
    context.perf.log(
        "lockfile_install",
        lockfile_started_at.elapsed(),
        &[("packages", lockfile.packages.len().to_string())],
    )?;

    finish_install(lockfile.packages.len(), context)
}

fn install_from_resolution(
    packages: Vec<RequiredPackage>,
    context: &InstallContext<'_>,
) -> Result<InstallSummary> {
    std::fs::create_dir_all(".concerto/store").map_err(|error| {
        ConcertoError::store(
            "root",
            StoreStep::Prepare,
            format!("could not create local store: {error}"),
        )
    })?;

    std::fs::create_dir_all("vendor").map_err(|error| {
        ConcertoError::store(
            "root",
            StoreStep::Prepare,
            format!("could not create vendor directory: {error}"),
        )
    })?;

    let mut speculative_preparer = resolved_install::SpeculativePreparer::new();
    let resolved_packages = resolver::resolve_with_observer(
        &packages,
        context.platform,
        context.perf,
        context.reporter,
        &mut speculative_preparer,
    )?;
    validate_resolved_platform_requirements(&resolved_packages, context.platform)?;
    let package_count = resolved_packages.len();
    resolved_install::install(
        &resolved_packages,
        context.perf,
        context.reporter,
        Some(speculative_preparer),
    )?;

    let lockfile = build_lockfile(packages, resolved_packages);
    write_autoload(&lockfile, context.root_composer_json, context.perf)?;
    context.reporter.emit(InstallEventKind::AutoloadWritten {
        packages: lockfile.packages.len(),
    });
    let lockfile_started_at = Instant::now();
    lockfile::write(&lockfile)?;
    context.reporter.emit(InstallEventKind::LockfileWritten);
    context
        .perf
        .log("lockfile_write", lockfile_started_at.elapsed(), &[])?;

    finish_install(package_count, context)
}

fn finish_install(package_count: usize, context: &InstallContext<'_>) -> Result<InstallSummary> {
    let install_duration = context.started_at.elapsed();
    context.perf.finish_run(install_duration, package_count)?;
    let summary = InstallSummary {
        packages: package_count,
        duration: install_duration,
    };

    Ok(summary)
}

fn write_autoload(lockfile: &Lockfile, root_composer_json: &str, perf: &PerfLogger) -> Result<()> {
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
) -> Result<()> {
    for package in packages {
        platform::validate(&package.platform_requires, platform, &package.name)?;
    }

    Ok(())
}

fn validate_resolved_platform_requirements(
    packages: &ResolvedPackages,
    platform: &platform::Platform,
) -> Result<()> {
    for (name, package) in packages {
        platform::validate(&package.platform_requires, platform, name)?;
    }

    Ok(())
}
fn prepare_package_source(
    name: &str,
    version: &str,
    dist_url: &str,
) -> Result<PackageSourcePreparation> {
    let archive = PackageArchive { version, dist_url };
    let started_at = Instant::now();
    let source = package_store::prepare_source(name, archive)?;
    let event = if source.is_reused() {
        "source_reuse"
    } else {
        "source_download_extract"
    };

    Ok(PackageSourcePreparation {
        source,
        duration: started_at.elapsed(),
        event,
    })
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
