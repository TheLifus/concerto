use super::{PackageSourcePreparation, prepare_package_source};
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

pub(super) fn install(packages: &[LockedPackage], perf: &PerfLogger) -> Result<(), String> {
    let prepare_started_at = Instant::now();
    let prepared_packages = prepare_sources(packages)?;

    perf.log(
        "lockfile_sources_prepare",
        prepare_started_at.elapsed(),
        &[("packages", prepared_packages.len().to_string())],
    )?;

    for package in prepared_packages {
        install_prepared_package(package, perf)?;
    }

    Ok(())
}

fn prepare_sources(packages: &[LockedPackage]) -> Result<Vec<PreparedLockedPackage>, String> {
    std::thread::scope(|scope| {
        let handles = packages
            .iter()
            .map(|package| scope.spawn(move || prepare_source(package)))
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

fn prepare_source(package: &LockedPackage) -> Result<PreparedLockedPackage, String> {
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
) -> Result<(), String> {
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

    println!(
        "{} {} -> {}",
        package.name,
        package.version,
        package.source.path().display()
    );

    Ok(())
}
