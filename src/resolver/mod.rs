use crate::composer::RequiredPackage;
use crate::error::{ConcertoError, Result};
use crate::http::get_text;
use crate::install_event::{InstallEventKind, InstallReporter};
use crate::packagist::{self, PackagistRelease};
use crate::perf::PerfLogger;
use crate::platform::Platform;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const PACKAGIST_FIXTURES_DIR_ENV: &str = "CONCERTO_PACKAGIST_FIXTURES_DIR";
const MAX_PARALLEL_RESOLVERS: usize = 8;

pub(crate) struct ResolvedPackageEntry {
    pub version: String,
    pub dist_url: String,
    pub constraints: Vec<String>,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

struct PackageResolveRequest {
    name: String,
    constraints: Vec<String>,
}

struct ResolvedPackage {
    name: String,
    constraints: Vec<String>,
    release: PackagistRelease,
    metadata_size: usize,
    duration: Duration,
}

struct ResolveState<'a> {
    package_constraints: &'a mut PackageConstraints,
    resolved_packages: &'a mut ResolvedPackages,
    pending: &'a mut Vec<String>,
    perf: &'a PerfLogger,
    reporter: &'a InstallReporter,
}

type PackageConstraints = HashMap<String, Vec<String>>;
pub(crate) type ResolvedPackages = HashMap<String, ResolvedPackageEntry>;

pub(crate) fn resolve(
    root_packages: &[RequiredPackage],
    platform: &Platform,
    perf: &PerfLogger,
    reporter: &InstallReporter,
) -> Result<ResolvedPackages> {
    let mut package_constraints = PackageConstraints::new();

    for package in root_packages {
        add_package_constraint(&mut package_constraints, package);
    }

    let mut resolved_packages = ResolvedPackages::new();
    let mut pending = root_packages
        .iter()
        .map(|package| package.name.clone())
        .collect::<Vec<_>>();

    while !pending.is_empty() {
        let requests =
            take_resolve_requests(&mut pending, &package_constraints, &resolved_packages)?;

        if requests.is_empty() {
            continue;
        }

        let mut state = ResolveState {
            package_constraints: &mut package_constraints,
            resolved_packages: &mut resolved_packages,
            pending: &mut pending,
            perf,
            reporter,
        };

        for package in resolve_package_batch(requests, platform)? {
            insert_resolved_package(package, &mut state)?;
        }
    }

    Ok(resolved_packages)
}

fn add_package_constraint(package_constraints: &mut PackageConstraints, package: &RequiredPackage) {
    package_constraints
        .entry(package.name.clone())
        .or_default()
        .push(package.constraint.clone());
}

fn take_resolve_requests(
    pending: &mut Vec<String>,
    package_constraints: &PackageConstraints,
    resolved_packages: &ResolvedPackages,
) -> Result<Vec<PackageResolveRequest>> {
    pending.sort();
    pending.dedup();

    let package_names = std::mem::take(pending);
    let mut requests = Vec::new();

    for name in package_names {
        let constraints = package_constraints.get(&name).ok_or_else(|| {
            ConcertoError::resolution(
                &name,
                &[],
                "missing constraints while resolving dependency graph",
            )
        })?;

        if let Some(resolved_package) = resolved_packages.get(&name) {
            ensure_resolved_package_matches(&name, constraints, resolved_package)?;
        } else {
            requests.push(PackageResolveRequest {
                name,
                constraints: constraints.clone(),
            });
        }
    }

    Ok(requests)
}

fn resolve_package_batch(
    requests: Vec<PackageResolveRequest>,
    platform: &Platform,
) -> Result<Vec<ResolvedPackage>> {
    let mut requests = requests.into_iter();
    let mut resolved = Vec::new();

    loop {
        let batch = requests
            .by_ref()
            .take(MAX_PARALLEL_RESOLVERS)
            .collect::<Vec<_>>();

        if batch.is_empty() {
            return Ok(resolved);
        }

        let mut batch = resolve_package_batch_chunk(batch, platform)?;
        resolved.append(&mut batch);
    }
}

fn resolve_package_batch_chunk(
    requests: Vec<PackageResolveRequest>,
    platform: &Platform,
) -> Result<Vec<ResolvedPackage>> {
    std::thread::scope(|scope| {
        let handles = requests
            .into_iter()
            .map(|request| scope.spawn(move || resolve_package(request, platform)))
            .collect::<Vec<_>>();

        let mut resolved = Vec::with_capacity(handles.len());

        for handle in handles {
            let package = handle
                .join()
                .map_err(|_| ConcertoError::internal("Package resolver worker panicked"))??;
            resolved.push(package);
        }

        Ok(resolved)
    })
}

fn resolve_package(request: PackageResolveRequest, platform: &Platform) -> Result<ResolvedPackage> {
    let started_at = Instant::now();
    let metadata = package_metadata(&request.name, &request.constraints)?;

    let release = packagist::first_release_candidate(
        &metadata,
        &request.name,
        &request.constraints,
        platform,
    )?;

    Ok(ResolvedPackage {
        name: request.name,
        constraints: request.constraints,
        release,
        metadata_size: metadata.len(),
        duration: started_at.elapsed(),
    })
}

fn package_metadata(package_name: &str, constraints: &[String]) -> Result<String> {
    if let Ok(fixtures_dir) = std::env::var(PACKAGIST_FIXTURES_DIR_ENV) {
        let path = fixture_metadata_path(Path::new(&fixtures_dir), package_name);
        let content = std::fs::read_to_string(&path).map_err(|error| {
            ConcertoError::resolution(
                package_name,
                constraints,
                format!(
                    "could not read Packagist fixture at {}: {}",
                    path.display(),
                    error
                ),
            )
        })?;

        return Ok(content);
    }

    let metadata_url = packagist::package_url(package_name)?;
    let metadata = get_text(&metadata_url).map_err(|error| {
        ConcertoError::resolution(
            package_name,
            constraints,
            format!("could not fetch Packagist metadata from {metadata_url}: {error}"),
        )
    })?;

    Ok(metadata)
}

fn fixture_metadata_path(fixtures_dir: &Path, package_name: &str) -> PathBuf {
    fixtures_dir.join(format!("{}.json", package_name.replace('/', "-")))
}

fn insert_resolved_package(resolved: ResolvedPackage, state: &mut ResolveState<'_>) -> Result<()> {
    state.reporter.emit(InstallEventKind::MetadataFetched {
        package: resolved.name.clone(),
        bytes: resolved.metadata_size,
    });
    state.perf.log(
        "resolve_package",
        resolved.duration,
        &[
            ("package", resolved.name.clone()),
            ("version", resolved.release.version.clone()),
        ],
    )?;
    state.reporter.emit(InstallEventKind::PackageResolved {
        package: resolved.name.clone(),
        version: resolved.release.version.clone(),
        version_count: resolved.release.version_count,
        package_requirements: resolved.release.package_requires.len(),
        platform_requirements: resolved.release.platform_requires.len(),
        dist_url: resolved.release.dist_url.clone(),
    });

    let package_requires = resolved.release.package_requires.clone();

    state.resolved_packages.insert(
        resolved.name.clone(),
        ResolvedPackageEntry {
            version: resolved.release.version.clone(),
            dist_url: resolved.release.dist_url.clone(),
            constraints: resolved.constraints,
            package_requires: resolved.release.package_requires,
            platform_requires: resolved.release.platform_requires,
        },
    );

    for requirement in package_requires {
        add_package_constraint(state.package_constraints, &requirement);
        state.pending.push(requirement.name);
    }

    Ok(())
}

fn ensure_resolved_package_matches(
    package_name: &str,
    constraints: &[String],
    resolved_package: &ResolvedPackageEntry,
) -> Result<()> {
    let mut satisfies = true;

    for constraint in constraints {
        let constraint_matches =
            semver_php::Semver::satisfies(&resolved_package.version, constraint).map_err(
                |error| {
                    ConcertoError::resolution(
                        package_name,
                        constraints,
                        format!("could not check installed package constraint: {error}"),
                    )
                },
            )?;
        satisfies = satisfies && constraint_matches;
    }

    if satisfies {
        return Ok(());
    }

    Err(ConcertoError::resolution(
        package_name,
        constraints,
        format!(
            "version conflict: resolved {} from {}, but requested {}",
            resolved_package.version,
            resolved_package.constraints.join(", "),
            constraints.join(", ")
        ),
    ))
}
