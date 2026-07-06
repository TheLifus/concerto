use super::{MAX_PARALLEL_WORKERS, PackageSourcePreparation, prepare_package_source};
use crate::error::{ConcertoError, Result};
use crate::install_event::{InstallEventKind, InstallReporter};
use crate::lockfile::LockedPackage;
use crate::package_store;
use crate::perf::PerfLogger;
use std::time::{Duration, Instant};

struct PreparedLockedPackage {
    name: String,
    version: String,
    source: package_store::PackageSource,
    source_duration: Duration,
    source_event: &'static str,
}

pub(super) fn install(
    packages: &[LockedPackage],
    perf: &PerfLogger,
    reporter: &InstallReporter,
) -> Result<()> {
    let prepare_started_at = Instant::now();
    let prepared_packages = prepare_sources(packages, reporter)?;

    perf.log(
        "lockfile_sources_prepare",
        prepare_started_at.elapsed(),
        &[("packages", prepared_packages.len().to_string())],
    )?;

    for package in prepared_packages {
        install_prepared_package(package, perf, reporter)?;
    }

    Ok(())
}

fn prepare_sources(
    packages: &[LockedPackage],
    reporter: &InstallReporter,
) -> Result<Vec<PreparedLockedPackage>> {
    let mut prepared = Vec::with_capacity(packages.len());

    for batch in packages.chunks(MAX_PARALLEL_WORKERS) {
        let mut batch = prepare_source_batch(batch)?;
        for package in &batch {
            emit_source_event(package, reporter);
        }
        prepared.append(&mut batch);
    }

    Ok(prepared)
}

fn prepare_source_batch(packages: &[LockedPackage]) -> Result<Vec<PreparedLockedPackage>> {
    std::thread::scope(|scope| {
        let handles = packages
            .iter()
            .map(|package| scope.spawn(move || prepare_source(package)))
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

fn prepare_source(package: &LockedPackage) -> Result<PreparedLockedPackage> {
    let PackageSourcePreparation {
        source,
        duration,
        event,
    } = prepare_package_source(&package.name, &package.version, &package.dist_url)?;

    Ok(PreparedLockedPackage {
        name: package.name.clone(),
        version: package.version.clone(),
        source,
        source_duration: duration,
        source_event: event,
    })
}

fn install_prepared_package(
    package: PreparedLockedPackage,
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

fn emit_source_event(package: &PreparedLockedPackage, reporter: &InstallReporter) {
    let path = InstallReporter::path(package.source.path());

    if package.source_event == "source_reuse" {
        reporter.emit(InstallEventKind::SourceReused {
            package: package.name.clone(),
            path,
        });
    } else {
        reporter.emit(InstallEventKind::SourcePrepared {
            package: package.name.clone(),
            path,
        });
    }
}
