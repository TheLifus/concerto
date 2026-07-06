use crate::composer::RequiredPackage;
use crate::http::get_text;
use crate::packagist::{self, PackagistRelease};
use crate::perf::PerfLogger;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub(crate) struct ResolvedPackageEntry {
    pub version: String,
    pub dist_url: String,
    pub metadata_url: String,
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
    metadata_url: String,
    metadata_size: usize,
    duration: Duration,
}

type PackageConstraints = HashMap<String, Vec<String>>;
pub(crate) type ResolvedPackages = HashMap<String, ResolvedPackageEntry>;

pub(crate) fn resolve(
    root_packages: &[RequiredPackage],
    perf: &PerfLogger,
) -> Result<ResolvedPackages, String> {
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

        for package in resolve_package_batch(requests)? {
            insert_resolved_package(
                package,
                &mut package_constraints,
                &mut resolved_packages,
                &mut pending,
                perf,
            )?;
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
) -> Result<Vec<PackageResolveRequest>, String> {
    pending.sort();
    pending.dedup();

    let package_names = std::mem::take(pending);
    let mut requests = Vec::new();

    for name in package_names {
        let constraints = package_constraints
            .get(&name)
            .ok_or_else(|| format!("Missing constraints for {name}"))?;

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
) -> Result<Vec<ResolvedPackage>, String> {
    std::thread::scope(|scope| {
        let handles = requests
            .into_iter()
            .map(|request| scope.spawn(move || resolve_package(request)))
            .collect::<Vec<_>>();

        let mut resolved = Vec::with_capacity(handles.len());

        for handle in handles {
            let package = handle
                .join()
                .map_err(|_| "Package resolver worker panicked".to_string())??;
            resolved.push(package);
        }

        Ok(resolved)
    })
}

fn resolve_package(request: PackageResolveRequest) -> Result<ResolvedPackage, String> {
    let started_at = Instant::now();
    let metadata_url = packagist::package_url(&request.name)?;
    let metadata = get_text(&metadata_url)?;

    let release =
        packagist::first_release_candidate(&metadata, &request.name, &request.constraints)?;

    Ok(ResolvedPackage {
        name: request.name,
        constraints: request.constraints,
        release,
        metadata_url,
        metadata_size: metadata.len(),
        duration: started_at.elapsed(),
    })
}

fn insert_resolved_package(
    resolved: ResolvedPackage,
    package_constraints: &mut PackageConstraints,
    resolved_packages: &mut ResolvedPackages,
    pending: &mut Vec<String>,
    perf: &PerfLogger,
) -> Result<(), String> {
    println!("Fetched {} bytes", resolved.metadata_size);
    perf.log(
        "resolve_package",
        resolved.duration,
        &[
            ("package", resolved.name.clone()),
            ("version", resolved.release.version.clone()),
        ],
    )?;
    print_release(&resolved.release);
    print_requirements(&resolved.release);

    let package_requires = resolved.release.package_requires.clone();

    resolved_packages.insert(
        resolved.name.clone(),
        ResolvedPackageEntry {
            version: resolved.release.version.clone(),
            dist_url: resolved.release.dist_url.clone(),
            metadata_url: resolved.metadata_url.clone(),
            constraints: resolved.constraints,
            package_requires: resolved.release.package_requires,
            platform_requires: resolved.release.platform_requires,
        },
    );

    for requirement in package_requires {
        add_package_constraint(package_constraints, &requirement);
        pending.push(requirement.name);
    }

    Ok(())
}

fn ensure_resolved_package_matches(
    package_name: &str,
    constraints: &[String],
    resolved_package: &ResolvedPackageEntry,
) -> Result<(), String> {
    let mut satisfies = true;

    for constraint in constraints {
        let constraint_matches =
            semver_php::Semver::satisfies(&resolved_package.version, constraint).map_err(
                |error| format!("Could not check installed package constraint: {error}"),
            )?;
        satisfies = satisfies && constraint_matches;
    }

    if satisfies {
        println!(
            "Skipping already installed {} {}",
            package_name, resolved_package.version
        );

        return Ok(());
    }

    Err(format!(
        "Version conflict for {}: resolved {} from {}, but requested {}",
        package_name,
        resolved_package.version,
        resolved_package.constraints.join(", "),
        constraints.join(", ")
    ))
}

fn print_release(release: &PackagistRelease) {
    println!("Found {} versions", release.version_count);
    println!("Selected {}", release.version);
    println!("Requires {} packages", release.package_requires.len());
    println!(
        "Requires {} platform packages",
        release.platform_requires.len()
    );
    println!("Dist {}", release.dist_url);
}

fn print_requirements(release: &PackagistRelease) {
    for requirement in &release.package_requires {
        println!(
            "Package requirement: {} {}",
            requirement.name, requirement.constraint
        );
    }

    for requirement in &release.platform_requires {
        println!(
            "Platform requirement: {} {}",
            requirement.name, requirement.constraint
        );
    }
}
