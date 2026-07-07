mod lockfile_install;
mod resolved_install;

use crate::autoload;
use crate::error::{ConcertoError, Result, StoreStep};
use crate::install_event::{InstallEventKind, InstallReporter, InstallSummary};
use crate::lockfile::{self, LockedPackage, Lockfile};

use crate::composer::{ComposerRepository, RequiredPackage, manifest};
use crate::package_store::{self, IntegrityCheckKind, PackageArchive, VendorLinkChange};
use crate::perf::PerfLogger;
use crate::platform;
use crate::resolver::{self, ResolveContext, ResolvedPackages};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::time::{Duration, Instant};

pub(super) const MAX_PARALLEL_WORKERS: usize = 16;

pub(super) struct PackageSourcePreparation {
    source: package_store::PackageSource,
    duration: Duration,
    event: &'static str,
}

pub(super) type PackageIntegrities = HashMap<String, String>;

struct InstallContext<'a> {
    root_composer_json: &'a str,
    platform: &'a platform::Platform,
    perf: &'a PerfLogger,
    reporter: &'a InstallReporter,
    started_at: Instant,
    unsafe_trust_store: bool,
}

struct ResolutionInstallRequest {
    packages: Vec<RequiredPackage>,
    root_requirements: Vec<RequiredPackage>,
    repositories: Vec<ComposerRepository>,
    production_requirements: Vec<RequiredPackage>,
    include_dev: bool,
}

pub fn install(
    reporter: InstallReporter,
    include_dev: bool,
    unsafe_trust_store: bool,
) -> Result<InstallSummary> {
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

    let manifest = manifest(&content)?;
    let root_requirements = manifest.root_requirements(true);
    let install_requirements = manifest.install_requirements(true);
    let production_requirements = manifest.install_requirements(false);

    if let Some(lockfile) = lockfile::read()? {
        if lockfile::matches_root_manifest(&lockfile, &root_requirements, &manifest.repositories) {
            let lockfile = active_lockfile(lockfile, include_dev);
            let platform_requirements =
                locked_platform_requirements(&manifest.platform_requirements, &lockfile.packages);
            let platform = detect_platform_for(&platform_requirements, &perf, &reporter)?;
            platform::validate(&manifest.platform_requirements, &platform, "root")?;
            let context = InstallContext {
                root_composer_json: &content,
                platform: &platform,
                perf: &perf,
                reporter: &reporter,
                started_at: install_started_at,
                unsafe_trust_store,
            };

            return install_from_lockfile(lockfile, &context);
        }

        reporter.emit(InstallEventKind::LockfileOutdated);
    }

    let platform = detect_platform(&perf, &reporter)?;
    platform::validate(&manifest.platform_requirements, &platform, "root")?;
    let context = InstallContext {
        root_composer_json: &content,
        platform: &platform,
        perf: &perf,
        reporter: &reporter,
        started_at: install_started_at,
        unsafe_trust_store,
    };

    install_from_resolution(
        ResolutionInstallRequest {
            packages: install_requirements,
            root_requirements,
            repositories: manifest.repositories,
            production_requirements,
            include_dev,
        },
        &context,
    )
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

    let link_changes = lockfile_install::install(
        &lockfile.packages,
        context.unsafe_trust_store,
        context.perf,
        context.reporter,
    )?;

    write_autoload_or_rollback(
        &lockfile,
        context.root_composer_json,
        context.perf,
        &link_changes,
    )?;
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
    request: ResolutionInstallRequest,
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

    let mut speculative_preparer =
        resolved_install::SpeculativePreparer::new(context.unsafe_trust_store);
    let resolve_context = ResolveContext {
        repositories: &request.repositories,
        platform: context.platform,
        perf: context.perf,
        reporter: context.reporter,
    };
    let resolved_packages = resolver::resolve_with_observer(
        &request.packages,
        &resolve_context,
        &mut speculative_preparer,
    )?;
    let production_packages =
        production_package_names(&request.production_requirements, &resolved_packages);
    let active_names = active_package_names(
        request.include_dev,
        &resolved_packages,
        &production_packages,
    );
    let active_resolved_packages = selected_resolved_packages(&resolved_packages, &active_names);
    validate_resolved_platform_requirements(&active_resolved_packages, context.platform)?;
    let package_count = active_resolved_packages.len();
    let (integrities, link_changes) = resolved_install::install(
        &resolved_packages,
        &active_names,
        context.perf,
        context.reporter,
        Some(speculative_preparer),
    )?;
    let lockfile = build_lockfile(
        request.root_requirements,
        request.repositories,
        &request.production_requirements,
        &resolved_packages,
        &integrities,
    )?;

    let previous_lockfile = read_lockfile_content()?;
    let lockfile_started_at = Instant::now();
    if let Err(error) = lockfile::write(&lockfile) {
        package_store::rollback_vendor_links(&link_changes)?;

        return Err(error);
    }
    let lockfile_duration = lockfile_started_at.elapsed();
    let active_lockfile = active_lockfile(lockfile.clone(), request.include_dev);
    if let Err(error) = write_autoload(&active_lockfile, context.root_composer_json, context.perf) {
        let restore_result = restore_lockfile_content(previous_lockfile);
        let rollback_result = package_store::rollback_vendor_links(&link_changes);

        restore_result?;
        rollback_result?;

        return Err(error);
    }
    context.reporter.emit(InstallEventKind::AutoloadWritten {
        packages: active_lockfile.packages.len(),
    });
    context.reporter.emit(InstallEventKind::LockfileWritten);
    context.perf.log("lockfile_write", lockfile_duration, &[])?;

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

fn read_lockfile_content() -> Result<Option<String>> {
    match std::fs::read_to_string(lockfile::LOCKFILE_PATH) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConcertoError::lockfile(format!(
            "Could not backup lockfile: {error}"
        ))),
    }
}

fn restore_lockfile_content(content: Option<String>) -> Result<()> {
    match content {
        Some(content) => std::fs::write(lockfile::LOCKFILE_PATH, content).map_err(|error| {
            ConcertoError::lockfile(format!("Could not restore lockfile: {error}"))
        }),
        None => match std::fs::remove_file(lockfile::LOCKFILE_PATH) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(ConcertoError::lockfile(format!(
                "Could not remove new lockfile: {error}"
            ))),
        },
    }
}

fn write_autoload_or_rollback(
    lockfile: &Lockfile,
    root_composer_json: &str,
    perf: &PerfLogger,
    link_changes: &[VendorLinkChange],
) -> Result<()> {
    match write_autoload(lockfile, root_composer_json, perf) {
        Ok(()) => Ok(()),
        Err(error) => {
            package_store::rollback_vendor_links(link_changes)?;

            Err(error)
        }
    }
}

pub(super) fn install_vendor_links<I, F>(items: I, mut install: F) -> Result<Vec<VendorLinkChange>>
where
    I: IntoIterator,
    F: FnMut(I::Item) -> Result<VendorLinkChange>,
{
    let mut link_changes = Vec::new();

    for item in items {
        match install(item) {
            Ok(change) => link_changes.push(change),
            Err(error) => {
                package_store::rollback_vendor_links(&link_changes)?;

                return Err(error);
            }
        }
    }

    Ok(link_changes)
}

fn detect_platform(perf: &PerfLogger, reporter: &InstallReporter) -> Result<platform::Platform> {
    let started_at = Instant::now();
    let platform = platform::current()?;

    emit_platform_detected(&platform, reporter);
    perf.log(
        "platform_current",
        started_at.elapsed(),
        &[("mode", "full".to_string())],
    )?;

    Ok(platform)
}

fn detect_platform_for(
    requirements: &[RequiredPackage],
    perf: &PerfLogger,
    reporter: &InstallReporter,
) -> Result<platform::Platform> {
    let started_at = Instant::now();
    let platform = platform::current_for(requirements)?;
    let mode = if platform::needs_extension_metadata(requirements) {
        "full"
    } else {
        "php"
    };

    emit_platform_detected(&platform, reporter);
    perf.log(
        "platform_current",
        started_at.elapsed(),
        &[("mode", mode.to_string())],
    )?;

    Ok(platform)
}

fn emit_platform_detected(platform: &platform::Platform, reporter: &InstallReporter) {
    reporter.emit(InstallEventKind::PlatformDetected {
        php_version: platform.php_version.clone(),
        extension_count: platform.extensions.len(),
    });
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

fn locked_platform_requirements(
    root_platform_requirements: &[RequiredPackage],
    packages: &[LockedPackage],
) -> Vec<RequiredPackage> {
    root_platform_requirements
        .iter()
        .chain(
            packages
                .iter()
                .flat_map(|package| package.platform_requires.iter()),
        )
        .cloned()
        .collect()
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
    archive: PackageArchive<'_>,
) -> Result<PackageSourcePreparation> {
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

fn log_integrity_check(
    check: Option<package_store::IntegrityCheck>,
    package: &str,
    perf: &PerfLogger,
) -> Result<()> {
    let Some(check) = check else {
        return Ok(());
    };
    let event = match check.kind {
        IntegrityCheckKind::DownloadHash => "archive_hash_download",
        IntegrityCheckKind::ReuseHash => "archive_hash_reuse",
        IntegrityCheckKind::UnsafeTrustedReuse => "archive_trust_reuse",
    };

    perf.log(
        event,
        check.duration,
        &[
            ("package", package.to_string()),
            ("sha1", check.sha1.to_string()),
        ],
    )
}

fn build_lockfile(
    root_requirements: Vec<RequiredPackage>,
    root_repositories: Vec<ComposerRepository>,
    production_requirements: &[RequiredPackage],
    resolved_packages: &ResolvedPackages,
    integrities: &PackageIntegrities,
) -> Result<Lockfile> {
    let mut root_requirements = root_requirements;
    let production_packages = production_package_names(production_requirements, resolved_packages);
    let mut packages = resolved_packages
        .iter()
        .map(|(name, package)| {
            let integrity = integrities.get(name).cloned().ok_or_else(|| {
                ConcertoError::internal(format!("Missing archive integrity for {name}"))
            })?;

            Ok(LockedPackage {
                name: name.clone(),
                version: package.version.clone(),
                dist_url: package.dist_url.clone(),
                dist_integrity: Some(integrity),
                dist_shasum: package.dist_shasum.clone(),
                dev: !production_packages.contains(name),
                package_requires: package.package_requires.clone(),
                platform_requires: package.platform_requires.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    root_requirements.sort_by(|left, right| left.name.cmp(&right.name));
    packages.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(Lockfile {
        lockfile_version: lockfile::LOCKFILE_VERSION,
        root_manifest_hash: lockfile::root_manifest_hash(&root_requirements, &root_repositories),
        root_requirements,
        root_repositories,
        packages,
    })
}

fn active_lockfile(lockfile: Lockfile, include_dev: bool) -> Lockfile {
    if include_dev {
        return lockfile;
    }

    Lockfile {
        packages: lockfile
            .packages
            .into_iter()
            .filter(|package| !package.dev)
            .collect(),
        ..lockfile
    }
}

fn active_package_names(
    include_dev: bool,
    resolved_packages: &ResolvedPackages,
    production_packages: &HashSet<String>,
) -> HashSet<String> {
    if include_dev {
        return resolved_packages.keys().cloned().collect();
    }

    production_packages.clone()
}

fn selected_resolved_packages(
    resolved_packages: &ResolvedPackages,
    active_names: &HashSet<String>,
) -> ResolvedPackages {
    resolved_packages
        .iter()
        .filter(|(name, _)| active_names.contains(name.as_str()))
        .map(|(name, package)| (name.clone(), package.clone()))
        .collect()
}

fn production_package_names(
    root_requirements: &[RequiredPackage],
    resolved_packages: &ResolvedPackages,
) -> HashSet<String> {
    let mut production_packages = HashSet::new();
    let mut pending = root_requirements
        .iter()
        .flat_map(|requirement| selected_package_names(requirement, resolved_packages))
        .collect::<Vec<_>>();

    while let Some(name) = pending.pop() {
        if !production_packages.insert(name.clone()) {
            continue;
        }

        let Some(package) = resolved_packages.get(&name) else {
            continue;
        };

        pending.extend(
            package
                .package_requires
                .iter()
                .flat_map(|requirement| selected_package_names(requirement, resolved_packages)),
        );
    }

    production_packages
}

fn selected_package_names(
    requirement: &RequiredPackage,
    resolved_packages: &ResolvedPackages,
) -> Vec<String> {
    resolved_packages
        .iter()
        .filter(|(name, package)| package_satisfies_requirement(name, package, requirement))
        .map(|(name, _)| name.clone())
        .collect()
}

fn package_satisfies_requirement(
    name: &str,
    package: &resolver::ResolvedPackageEntry,
    requirement: &RequiredPackage,
) -> bool {
    name == requirement.name
        || package
            .provides
            .iter()
            .chain(&package.replaces)
            .any(|capability| capability.name == requirement.name)
}

#[cfg(test)]
mod tests;
