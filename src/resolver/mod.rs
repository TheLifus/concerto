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
const MAX_PARALLEL_CANDIDATE_FETCHES: usize = 8;

#[derive(Clone, Debug)]
pub(crate) struct ResolvedPackageEntry {
    pub version: String,
    pub dist_url: String,
    pub constraints: Vec<String>,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

struct CandidateSet {
    releases: Vec<PackagistRelease>,
    metadata_size: usize,
    duration: Duration,
}

#[derive(Default)]
struct SolverStats {
    decisions: usize,
    backtracks: usize,
}

struct CandidateLoader<'a> {
    platform: &'a Platform,
    perf: &'a PerfLogger,
    reporter: &'a InstallReporter,
    metadata_cache: HashMap<String, String>,
    candidate_cache: CandidateCache,
}

struct CandidateFetch {
    key: CandidateCacheKey,
    metadata: Option<String>,
    candidates: CandidateSet,
}

#[derive(Clone)]
struct CandidateFetchJob {
    key: CandidateCacheKey,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct CandidateCacheKey {
    package_name: String,
    constraints: Vec<String>,
}

trait CandidateProvider {
    fn prefetch(
        &mut self,
        constraints: &PackageConstraints,
        selected: &SelectedPackages,
    ) -> Result<()>;

    fn load(&mut self, package_name: &str, constraints: &[String])
    -> Result<Vec<PackagistRelease>>;
}

type CandidateCache = HashMap<CandidateCacheKey, CandidateSet>;
type PackageConstraints = HashMap<String, Vec<String>>;
type SelectedPackages = HashMap<String, PackagistRelease>;
pub(crate) type ResolvedPackages = HashMap<String, ResolvedPackageEntry>;

pub(crate) trait ResolutionObserver {
    fn package_selected(&mut self, package_name: &str, release: &PackagistRelease);
}

pub(crate) fn resolve_with_observer(
    root_packages: &[RequiredPackage],
    platform: &Platform,
    perf: &PerfLogger,
    reporter: &InstallReporter,
    observer: &mut impl ResolutionObserver,
) -> Result<ResolvedPackages> {
    let package_constraints = root_package_constraints(root_packages);
    let mut candidate_loader = CandidateLoader {
        platform,
        perf,
        reporter,
        metadata_cache: HashMap::new(),
        candidate_cache: CandidateCache::new(),
    };
    let mut stats = SolverStats::default();
    let solver_started_at = Instant::now();

    let resolved_packages = solve_dependency_graph(
        package_constraints,
        &mut candidate_loader,
        &mut stats,
        observer,
    )?;

    perf.log(
        "resolve_solver",
        solver_started_at.elapsed(),
        &[
            ("packages", resolved_packages.len().to_string()),
            (
                "candidate_fetches",
                candidate_loader.candidate_cache.len().to_string(),
            ),
            ("decisions", stats.decisions.to_string()),
            ("backtracks", stats.backtracks.to_string()),
        ],
    )?;
    emit_resolved_package_events(
        &resolved_packages,
        &candidate_loader.candidate_cache,
        perf,
        reporter,
    )?;

    Ok(resolved_packages)
}

fn root_package_constraints(root_packages: &[RequiredPackage]) -> PackageConstraints {
    let mut package_constraints = PackageConstraints::new();

    for package in root_packages {
        add_package_constraint(&mut package_constraints, package);
    }

    package_constraints
}

fn add_package_constraint(package_constraints: &mut PackageConstraints, package: &RequiredPackage) {
    package_constraints
        .entry(package.name.clone())
        .or_default()
        .push(package.constraint.clone());
}

fn solve_dependency_graph(
    package_constraints: PackageConstraints,
    candidates: &mut impl CandidateProvider,
    stats: &mut SolverStats,
    observer: &mut impl ResolutionObserver,
) -> Result<ResolvedPackages> {
    let graph_constraints = graph_constraints(&package_constraints);
    let Some((constraints, selected)) = solve_next(
        package_constraints,
        SelectedPackages::new(),
        candidates,
        stats,
        observer,
    )?
    else {
        return Err(ConcertoError::resolution(
            "dependency graph",
            &graph_constraints,
            "no compatible package versions found",
        ));
    };

    Ok(resolved_package_entries(selected, &constraints))
}

fn solve_next(
    constraints: PackageConstraints,
    selected: SelectedPackages,
    candidates: &mut impl CandidateProvider,
    stats: &mut SolverStats,
    observer: &mut impl ResolutionObserver,
) -> Result<Option<(PackageConstraints, SelectedPackages)>> {
    candidates.prefetch(&constraints, &selected)?;

    let Some(package_name) = next_unresolved_package(&constraints, &selected) else {
        return Ok(Some((constraints, selected)));
    };
    let package_constraints = constraints.get(&package_name).ok_or_else(|| {
        ConcertoError::resolution(
            &package_name,
            &[],
            "missing constraints while resolving dependency graph",
        )
    })?;
    let releases = matching_releases(&package_name, package_constraints, candidates)?;

    for release in releases {
        stats.decisions += 1;

        // ponytail: branch cloning is simple and deterministic; switch to undo logs if graphs get large.
        let mut next_constraints = constraints.clone();
        let mut next_selected = selected.clone();

        for requirement in &release.package_requires {
            add_package_constraint(&mut next_constraints, requirement);
        }

        next_selected.insert(package_name.clone(), release);

        if !selected_packages_match_constraints(&next_selected, &next_constraints)? {
            stats.backtracks += 1;
            continue;
        }

        let selected_release = next_selected.get(&package_name).ok_or_else(|| {
            ConcertoError::internal("Selected package disappeared during resolution")
        })?;
        observer.package_selected(&package_name, selected_release);

        if let Some(solution) =
            solve_next(next_constraints, next_selected, candidates, stats, observer)?
        {
            return Ok(Some(solution));
        }

        stats.backtracks += 1;
    }

    Ok(None)
}

fn next_unresolved_package(
    constraints: &PackageConstraints,
    selected: &SelectedPackages,
) -> Option<String> {
    let mut package_names = constraints
        .keys()
        .filter(|package_name| !selected.contains_key(*package_name))
        .cloned()
        .collect::<Vec<_>>();
    package_names.sort();
    package_names.into_iter().next()
}

fn matching_releases(
    package_name: &str,
    constraints: &[String],
    candidates: &mut impl CandidateProvider,
) -> Result<Vec<PackagistRelease>> {
    let mut releases = Vec::new();

    for release in candidates.load(package_name, constraints)? {
        if release.matches_constraints(package_name, constraints)? {
            releases.push(release);
        }
    }

    Ok(releases)
}

fn selected_packages_match_constraints(
    selected: &SelectedPackages,
    constraints: &PackageConstraints,
) -> Result<bool> {
    for (package_name, release) in selected {
        let Some(package_constraints) = constraints.get(package_name) else {
            continue;
        };

        if !release.matches_constraints(package_name, package_constraints)? {
            return Ok(false);
        }
    }

    Ok(true)
}

fn resolved_package_entries(
    selected: SelectedPackages,
    constraints: &PackageConstraints,
) -> ResolvedPackages {
    selected
        .into_iter()
        .map(|(name, release)| {
            let package_constraints = constraints.get(&name).cloned().unwrap_or_default();

            (
                name,
                ResolvedPackageEntry {
                    version: release.version,
                    dist_url: release.dist_url,
                    constraints: package_constraints,
                    package_requires: release.package_requires,
                    platform_requires: release.platform_requires,
                },
            )
        })
        .collect()
}

fn graph_constraints(package_constraints: &PackageConstraints) -> Vec<String> {
    let mut constraints = package_constraints
        .iter()
        .map(|(package, constraints)| format!("{package} {}", constraints.join(", ")))
        .collect::<Vec<_>>();
    constraints.sort();
    constraints
}

impl CandidateProvider for CandidateLoader<'_> {
    fn prefetch(
        &mut self,
        constraints: &PackageConstraints,
        selected: &SelectedPackages,
    ) -> Result<()> {
        let jobs = missing_candidate_jobs(&self.candidate_cache, constraints, selected);

        for batch in jobs.chunks(MAX_PARALLEL_CANDIDATE_FETCHES) {
            for fetch in self.fetch_candidate_batch(batch)? {
                self.record(fetch)?;
            }
        }

        Ok(())
    }

    fn load(
        &mut self,
        package_name: &str,
        constraints: &[String],
    ) -> Result<Vec<PackagistRelease>> {
        let key = candidate_cache_key(package_name, constraints);

        if !self.candidate_cache.contains_key(&key) {
            let fetch = self.fetch(key.clone())?;
            self.record(fetch)?;
        }

        self.candidate_cache
            .get(&key)
            .map(|candidates| candidates.releases.clone())
            .ok_or_else(|| ConcertoError::internal("Candidate cache entry disappeared"))
    }
}

impl CandidateLoader<'_> {
    fn fetch(&self, key: CandidateCacheKey) -> Result<CandidateFetch> {
        if let Some(metadata) = self.metadata_cache.get(&key.package_name) {
            return parse_candidate_metadata(key, metadata, self.platform, None, Instant::now());
        }

        fetch_candidate(key, self.platform)
    }

    fn record(&mut self, fetch: CandidateFetch) -> Result<()> {
        if let Some(metadata) = &fetch.metadata {
            self.metadata_cache
                .insert(fetch.key.package_name.clone(), metadata.clone());
        }

        self.reporter.emit(InstallEventKind::MetadataFetched {
            package: fetch.key.package_name.clone(),
            bytes: fetch.candidates.metadata_size,
        });
        self.perf.log(
            "resolve_candidates",
            fetch.candidates.duration,
            &[
                ("package", fetch.key.package_name.clone()),
                ("bytes", fetch.candidates.metadata_size.to_string()),
                ("candidates", fetch.candidates.releases.len().to_string()),
                ("constraints", fetch.key.constraints.join(",")),
                (
                    "versions",
                    fetch
                        .candidates
                        .releases
                        .first()
                        .map(|release| release.version_count)
                        .unwrap_or_default()
                        .to_string(),
                ),
            ],
        )?;
        self.candidate_cache.insert(fetch.key, fetch.candidates);

        Ok(())
    }

    fn fetch_candidate_batch(&self, jobs: &[CandidateFetchJob]) -> Result<Vec<CandidateFetch>> {
        let mut fetched = Vec::with_capacity(jobs.len());
        let mut network_jobs = Vec::new();

        for job in jobs {
            if let Some(metadata) = self.metadata_cache.get(&job.key.package_name) {
                fetched.push(parse_candidate_metadata(
                    job.key.clone(),
                    metadata,
                    self.platform,
                    None,
                    Instant::now(),
                )?);
            } else {
                network_jobs.push(job.clone());
            }
        }

        fetched.append(&mut fetch_candidate_batch(&network_jobs, self.platform)?);

        Ok(fetched)
    }
}

fn candidate_cache_key(package_name: &str, constraints: &[String]) -> CandidateCacheKey {
    let mut constraints = constraints.to_vec();
    constraints.sort();
    constraints.dedup();

    CandidateCacheKey {
        package_name: package_name.to_string(),
        constraints,
    }
}

fn missing_candidate_jobs(
    candidate_cache: &CandidateCache,
    constraints: &PackageConstraints,
    selected: &SelectedPackages,
) -> Vec<CandidateFetchJob> {
    let mut jobs = constraints
        .iter()
        .map(|(package_name, constraints)| CandidateFetchJob {
            key: candidate_cache_key(package_name, constraints),
        })
        .filter(|job| {
            !selected.contains_key(&job.key.package_name) && !candidate_cache.contains_key(&job.key)
        })
        .collect::<Vec<_>>();
    jobs.sort_by(|left, right| left.key.package_name.cmp(&right.key.package_name));
    jobs
}

fn fetch_candidate_batch(
    jobs: &[CandidateFetchJob],
    platform: &Platform,
) -> Result<Vec<CandidateFetch>> {
    std::thread::scope(|scope| {
        let handles = jobs
            .iter()
            .map(|job| scope.spawn(move || fetch_candidate(job.key.clone(), platform)))
            .collect::<Vec<_>>();

        let mut fetched = Vec::with_capacity(handles.len());

        for handle in handles {
            fetched.push(
                handle
                    .join()
                    .map_err(|_| ConcertoError::internal("Candidate fetch worker panicked"))??,
            );
        }

        Ok(fetched)
    })
}

fn fetch_candidate(key: CandidateCacheKey, platform: &Platform) -> Result<CandidateFetch> {
    let started_at = Instant::now();
    let metadata = package_metadata(&key.package_name, &key.constraints)?;
    let releases =
        packagist::release_candidates(&metadata, &key.package_name, &key.constraints, platform)?;
    let metadata_size = metadata.len();

    Ok(CandidateFetch {
        key,
        metadata: Some(metadata),
        candidates: CandidateSet {
            releases,
            metadata_size,
            duration: started_at.elapsed(),
        },
    })
}

fn parse_candidate_metadata(
    key: CandidateCacheKey,
    metadata: &str,
    platform: &Platform,
    owned_metadata: Option<String>,
    started_at: Instant,
) -> Result<CandidateFetch> {
    let releases =
        packagist::release_candidates(metadata, &key.package_name, &key.constraints, platform)?;

    Ok(CandidateFetch {
        key,
        metadata: owned_metadata,
        candidates: CandidateSet {
            releases,
            metadata_size: metadata.len(),
            duration: started_at.elapsed(),
        },
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

fn emit_resolved_package_events(
    resolved_packages: &ResolvedPackages,
    candidate_cache: &CandidateCache,
    perf: &PerfLogger,
    reporter: &InstallReporter,
) -> Result<()> {
    let mut package_names = resolved_packages.keys().cloned().collect::<Vec<_>>();
    package_names.sort();

    for package_name in package_names {
        let resolved_package = resolved_packages.get(&package_name).ok_or_else(|| {
            ConcertoError::internal("Resolved package disappeared while reporting")
        })?;
        let candidates = resolved_candidate_set(&package_name, resolved_package, candidate_cache)?;

        perf.log(
            "resolve_package",
            candidates.duration,
            &[
                ("package", package_name.clone()),
                ("version", resolved_package.version.clone()),
                ("candidates", candidates.releases.len().to_string()),
                ("bytes", candidates.metadata_size.to_string()),
                ("constraints", resolved_package.constraints.join(",")),
            ],
        )?;
        reporter.emit(InstallEventKind::PackageResolved {
            package: package_name,
            version: resolved_package.version.clone(),
            version_count: candidates
                .releases
                .first()
                .map(|release| release.version_count)
                .unwrap_or_default(),
            package_requirements: resolved_package.package_requires.len(),
            platform_requirements: resolved_package.platform_requires.len(),
            dist_url: resolved_package.dist_url.clone(),
        });
    }

    Ok(())
}

fn resolved_candidate_set<'a>(
    package_name: &str,
    resolved_package: &ResolvedPackageEntry,
    candidate_cache: &'a CandidateCache,
) -> Result<&'a CandidateSet> {
    let key = candidate_cache_key(package_name, &resolved_package.constraints);

    if let Some(candidates) = candidate_cache.get(&key) {
        return Ok(candidates);
    }

    candidate_cache
        .iter()
        .find(|(key, candidates)| {
            key.package_name == package_name
                && candidates
                    .releases
                    .iter()
                    .any(|release| release.version == resolved_package.version)
        })
        .map(|(_, candidates)| candidates)
        .ok_or_else(|| ConcertoError::internal("Missing candidates for resolved package"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backtracks_when_later_constraint_rejects_first_candidate() {
        let mut stats = SolverStats::default();
        let mut candidates = InMemoryCandidates::new([
            (
                "acme/app",
                vec![
                    release("1.1.0", &[required_package("psr/log", "^3.0")]),
                    release("1.0.0", &[required_package("psr/log", "^2.0")]),
                ],
            ),
            (
                "psr/log",
                vec![release("3.0.0", &[]), release("2.0.0", &[])],
            ),
        ]);

        let resolved = solve_dependency_graph(
            root_package_constraints(&[
                required_package("acme/app", "^1.0"),
                required_package("psr/log", "^2.0"),
            ]),
            &mut candidates,
            &mut stats,
            &mut InMemoryObserver,
        )
        .unwrap();

        assert_eq!(resolved["acme/app"].version, "1.0.0");
        assert_eq!(resolved["psr/log"].version, "2.0.0");
        assert_eq!(
            resolved["psr/log"].constraints,
            vec!["^2.0".to_string(), "^2.0".to_string()]
        );
        assert!(stats.backtracks > 0);
        assert!(candidates.prefetches > 0);
    }

    #[test]
    fn returns_actionable_error_when_graph_has_no_solution() {
        let mut stats = SolverStats::default();
        let mut candidates = InMemoryCandidates::new([
            (
                "acme/app",
                vec![release("1.0.0", &[required_package("psr/log", "^3.0")])],
            ),
            ("psr/log", vec![release("2.0.0", &[])]),
        ]);

        let error = solve_dependency_graph(
            root_package_constraints(&[
                required_package("acme/app", "^1.0"),
                required_package("psr/log", "^2.0"),
            ]),
            &mut candidates,
            &mut stats,
            &mut InMemoryObserver,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("dependency graph"));
        assert!(error.contains("acme/app ^1.0"));
        assert!(error.contains("psr/log ^2.0"));
        assert!(error.contains("no compatible package versions found"));
    }

    struct InMemoryCandidates {
        packages: HashMap<String, Vec<PackagistRelease>>,
        prefetches: usize,
    }

    struct InMemoryObserver;

    impl ResolutionObserver for InMemoryObserver {
        fn package_selected(&mut self, _package_name: &str, _release: &PackagistRelease) {}
    }

    impl InMemoryCandidates {
        fn new<const N: usize>(packages: [(&str, Vec<PackagistRelease>); N]) -> Self {
            Self {
                packages: packages
                    .into_iter()
                    .map(|(name, releases)| (name.to_string(), releases))
                    .collect(),
                prefetches: 0,
            }
        }
    }

    impl CandidateProvider for InMemoryCandidates {
        fn prefetch(
            &mut self,
            _constraints: &PackageConstraints,
            _selected: &SelectedPackages,
        ) -> Result<()> {
            self.prefetches += 1;

            Ok(())
        }

        fn load(
            &mut self,
            package_name: &str,
            _constraints: &[String],
        ) -> Result<Vec<PackagistRelease>> {
            Ok(self.packages.get(package_name).cloned().unwrap_or_default())
        }
    }

    fn release(version: &str, package_requires: &[RequiredPackage]) -> PackagistRelease {
        PackagistRelease {
            version_count: 2,
            version: version.to_string(),
            dist_url: format!("https://example.com/{version}.zip"),
            package_requires: package_requires.to_vec(),
            platform_requires: Vec::new(),
        }
    }

    fn required_package(name: &str, constraint: &str) -> RequiredPackage {
        RequiredPackage {
            name: name.to_string(),
            constraint: constraint.to_string(),
        }
    }
}
