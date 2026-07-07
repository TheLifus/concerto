use super::{
    MAX_PARALLEL_WORKERS, PackageIntegrities, PackageSourcePreparation, install_vendor_links,
    log_integrity_check, prepare_package_source,
};
use crate::error::{ConcertoError, Result};
use crate::install_event::{InstallEventKind, InstallReporter};
use crate::package_store::{self, PackageArchive, VendorLinkChange};
use crate::packagist::PackagistRelease;
use crate::perf::PerfLogger;
use crate::resolver::{ResolutionObserver, ResolvedPackageEntry, ResolvedPackages};
use std::collections::{HashMap, HashSet};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

struct PreparedPackage {
    name: String,
    version: String,
    source: package_store::PackageSource,
    source_duration: Duration,
    source_event: &'static str,
}

pub(super) struct SpeculativePreparer {
    handles: HashMap<PackageKey, JoinHandle<Result<PreparedPackage>>>,
    unsafe_trust_store: bool,
}

#[derive(Hash, Eq, PartialEq)]
struct PackageKey {
    name: String,
    version: String,
}

pub(super) fn install(
    resolved_packages: &ResolvedPackages,
    active_names: &HashSet<String>,
    perf: &PerfLogger,
    reporter: &InstallReporter,
    speculative_preparer: Option<SpeculativePreparer>,
) -> Result<(PackageIntegrities, Vec<VendorLinkChange>)> {
    let prepare_started_at = Instant::now();
    let prepared_packages = prepare_sources(resolved_packages, reporter, speculative_preparer)?;
    let integrities = prepared_packages
        .iter()
        .map(|package| (package.name.clone(), package.source.integrity().to_string()))
        .collect::<PackageIntegrities>();
    for package in &prepared_packages {
        log_integrity_check(package.source.integrity_check(), &package.name, perf)?;
    }

    perf.log(
        "sources_prepare",
        prepare_started_at.elapsed(),
        &[("packages", prepared_packages.len().to_string())],
    )?;

    let link_changes = install_vendor_links(
        prepared_packages
            .iter()
            .filter(|package| active_names.contains(&package.name)),
        |package| install_prepared_package(package, perf, reporter),
    )?;

    Ok((integrities, link_changes))
}

impl SpeculativePreparer {
    pub(super) fn new(unsafe_trust_store: bool) -> Self {
        Self {
            handles: HashMap::new(),
            unsafe_trust_store,
        }
    }

    fn take(&mut self, name: &str, version: &str) -> Option<JoinHandle<Result<PreparedPackage>>> {
        self.handles.remove(&PackageKey {
            name: name.to_string(),
            version: version.to_string(),
        })
    }
}

impl Drop for SpeculativePreparer {
    fn drop(&mut self) {
        for (_, handle) in std::mem::take(&mut self.handles) {
            let _ = handle.join();
        }
    }
}

impl ResolutionObserver for SpeculativePreparer {
    fn package_selected(&mut self, package_name: &str, release: &PackagistRelease) {
        let key = PackageKey {
            name: package_name.to_string(),
            version: release.version.clone(),
        };

        if self.handles.contains_key(&key) {
            return;
        }

        let name = package_name.to_string();
        let version = release.version.clone();
        let dist_url = release.dist_url.clone();
        let dist_shasum = release.dist_shasum.clone();
        let unsafe_trust_store = self.unsafe_trust_store;
        let handle = std::thread::spawn(move || {
            let package = ResolvedPackageEntry {
                version,
                dist_url,
                dist_shasum,
                constraints: Vec::new(),
                package_requires: Vec::new(),
                platform_requires: Vec::new(),
                provides: Vec::new(),
                replaces: Vec::new(),
            };

            prepare_source(&name, &package, unsafe_trust_store)
        });

        self.handles.insert(key, handle);
    }
}

fn install_prepared_package(
    package: &PreparedPackage,
    perf: &PerfLogger,
    reporter: &InstallReporter,
) -> Result<VendorLinkChange> {
    perf.log(
        package.source_event,
        package.source_duration,
        &[("package", package.name.clone())],
    )?;

    let link_started_at = Instant::now();
    let link_change = package_store::link_to_vendor(&package.source)?;
    perf.log(
        "vendor_link",
        link_started_at.elapsed(),
        &[("package", package.name.clone())],
    )?;
    reporter.emit(InstallEventKind::VendorLinked {
        package: package.name.clone(),
        version: package.version.clone(),
        path: InstallReporter::path(package.source.path()),
    });

    Ok(link_change)
}

fn emit_source_event(package: &PreparedPackage, reporter: &InstallReporter) {
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

fn prepare_sources(
    resolved_packages: &ResolvedPackages,
    reporter: &InstallReporter,
    mut speculative_preparer: Option<SpeculativePreparer>,
) -> Result<Vec<PreparedPackage>> {
    let unsafe_trust_store = speculative_preparer
        .as_ref()
        .is_some_and(|preparer| preparer.unsafe_trust_store);
    let mut packages = resolved_packages.iter().collect::<Vec<_>>();

    packages.sort_by(|left, right| left.0.cmp(right.0));

    let mut prepared = Vec::with_capacity(packages.len());

    let mut missing = Vec::new();

    for (name, package) in packages {
        if let Some(handle) = speculative_preparer
            .as_mut()
            .and_then(|preparer| preparer.take(name, &package.version))
        {
            let package = join_prepared_package(handle)?;
            emit_source_event(&package, reporter);
            prepared.push(package);
        } else {
            missing.push((name, package));
        }
    }

    for batch in missing.chunks(MAX_PARALLEL_WORKERS) {
        let mut batch = prepare_source_batch(batch, unsafe_trust_store)?;
        for package in &batch {
            emit_source_event(package, reporter);
        }
        prepared.append(&mut batch);
    }

    Ok(prepared)
}

fn join_prepared_package(handle: JoinHandle<Result<PreparedPackage>>) -> Result<PreparedPackage> {
    handle
        .join()
        .map_err(|_| ConcertoError::internal("Speculative package source worker panicked"))?
}

fn prepare_source_batch(
    packages: &[(&String, &ResolvedPackageEntry)],
    unsafe_trust_store: bool,
) -> Result<Vec<PreparedPackage>> {
    std::thread::scope(|scope| {
        let handles = packages
            .iter()
            .map(|&(name, package)| {
                scope.spawn(move || prepare_source(name, package, unsafe_trust_store))
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
    unsafe_trust_store: bool,
) -> Result<PreparedPackage> {
    let PackageSourcePreparation {
        source,
        duration,
        event,
    } = prepare_package_source(
        name,
        PackageArchive {
            version: &package.version,
            dist_url: &package.dist_url,
            expected_integrity: None,
            expected_shasum: package.dist_shasum.as_deref(),
            unsafe_trust_store,
        },
    )?;

    Ok(PreparedPackage {
        name: name.to_string(),
        version: package.version.clone(),
        source,
        source_duration: duration,
        source_event: event,
    })
}
