use super::{MAX_PARALLEL_WORKERS, PackageSourcePreparation, prepare_package_source};
use crate::error::{ConcertoError, Result};
use crate::install_event::{InstallEventKind, InstallReporter};
use crate::package_store;
use crate::perf::PerfLogger;
use crate::resolver::{ResolvedPackageEntry, ResolvedPackages};
use std::time::{Duration, Instant};

struct PreparedPackage {
    name: String,
    version: String,
    source: package_store::PackageSource,
    source_duration: Duration,
    source_event: &'static str,
}

pub(super) fn install(
    resolved_packages: &ResolvedPackages,
    perf: &PerfLogger,
    reporter: &InstallReporter,
) -> Result<()> {
    let prepare_started_at = Instant::now();
    let prepared_packages = prepare_sources(resolved_packages, reporter)?;

    perf.log(
        "sources_prepare",
        prepare_started_at.elapsed(),
        &[("packages", prepared_packages.len().to_string())],
    )?;

    for package in prepared_packages {
        install_prepared_package(package, perf, reporter)?;
    }

    Ok(())
}

fn install_prepared_package(
    package: PreparedPackage,
    perf: &PerfLogger,
    reporter: &InstallReporter,
) -> Result<()> {
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
    reporter.emit(InstallEventKind::VendorLinked {
        package: package.name,
        version: package.version,
        path: InstallReporter::path(package.source.path()),
    });

    Ok(())
}

fn prepare_sources(
    resolved_packages: &ResolvedPackages,
    reporter: &InstallReporter,
) -> Result<Vec<PreparedPackage>> {
    let mut packages = resolved_packages.iter().collect::<Vec<_>>();

    packages.sort_by(|left, right| left.0.cmp(right.0));

    let mut prepared = Vec::with_capacity(packages.len());

    for batch in packages.chunks(MAX_PARALLEL_WORKERS) {
        let mut batch = prepare_source_batch(batch, reporter)?;
        prepared.append(&mut batch);
    }

    Ok(prepared)
}

fn prepare_source_batch(
    packages: &[(&String, &ResolvedPackageEntry)],
    reporter: &InstallReporter,
) -> Result<Vec<PreparedPackage>> {
    std::thread::scope(|scope| {
        let handles = packages
            .iter()
            .map(|&(name, package)| {
                let reporter = reporter.clone();

                scope.spawn(move || prepare_source(name, package, &reporter))
            })
            .collect::<Vec<_>>();

        let mut prepared = Vec::with_capacity(handles.len());

        for handle in handles {
            let package = handle
                .join()
                .map_err(|_| ConcertoError::internal("Package source worker panicked"))??;
            prepared.push(package);
        }

        Ok(prepared)
    })
}

fn prepare_source(
    name: &str,
    package: &ResolvedPackageEntry,
    reporter: &InstallReporter,
) -> Result<PreparedPackage> {
    let PackageSourcePreparation {
        source,
        duration,
        event,
    } = prepare_package_source(name, &package.version, &package.dist_url, reporter)?;

    Ok(PreparedPackage {
        name: name.to_string(),
        version: package.version.clone(),
        source,
        source_duration: duration,
        source_event: event,
    })
}
