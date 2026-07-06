use super::{PackageSourcePreparation, prepare_package_source, print_install_summary};
use crate::error::{ConcertoError, Result};
use crate::package_store;
use crate::perf::PerfLogger;
use crate::resolver::{ResolvedPackageEntry, ResolvedPackages};
use std::time::{Duration, Instant};

struct PreparedPackage {
    name: String,
    constraint: String,
    metadata_url: String,
    source: package_store::PackageSource,
    source_duration: Duration,
    source_event: &'static str,
}

pub(super) fn install(resolved_packages: &ResolvedPackages, perf: &PerfLogger) -> Result<()> {
    let prepare_started_at = Instant::now();
    let prepared_packages = prepare_sources(resolved_packages)?;

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

fn install_prepared_package(package: PreparedPackage, perf: &PerfLogger) -> Result<()> {
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

fn prepare_sources(resolved_packages: &ResolvedPackages) -> Result<Vec<PreparedPackage>> {
    let mut packages = resolved_packages.iter().collect::<Vec<_>>();

    packages.sort_by(|left, right| left.0.cmp(right.0));

    std::thread::scope(|scope| {
        let handles = packages
            .into_iter()
            .map(|(name, package)| scope.spawn(move || prepare_source(name, package)))
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

fn prepare_source(name: &str, package: &ResolvedPackageEntry) -> Result<PreparedPackage> {
    let PackageSourcePreparation {
        source,
        duration,
        event,
    } = prepare_package_source(name, &package.version, &package.dist_url)?;

    Ok(PreparedPackage {
        name: name.to_string(),
        constraint: package.constraints.join(", "),
        metadata_url: package.metadata_url.clone(),
        source,
        source_duration: duration,
        source_event: event,
    })
}
