use crate::composer::{ComposerRepository, RequiredPackage, is_package_name};
use crate::error::{ConcertoError, Result};
use crate::http::get_text;
use crate::install_event::{InstallEventKind, InstallReporter};
use crate::packagist::{self, PackagistRelease};
use crate::perf::PerfLogger;
use crate::platform::Platform;
use pubgrub::{
    DefaultStringReporter, Dependencies, DependencyConstraints, DependencyProvider,
    PackageResolutionStatistics, PubGrubError, Reporter, VersionSet, resolve,
};
use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::convert::Infallible;
use std::fmt;
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
    pub provides: Vec<RequiredPackage>,
    pub replaces: Vec<RequiredPackage>,
}

struct CandidateSet {
    releases: Vec<PackagistRelease>,
    metadata_size: usize,
    duration: Duration,
}

#[derive(Default)]
struct SolverStats {
    versions: usize,
    dependencies: usize,
    provider_versions: usize,
}

struct CandidateLoader<'a> {
    repositories: &'a [ComposerRepository],
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

    fn load_capability_providers(
        &mut self,
        requirement: &RequiredPackage,
    ) -> Result<Vec<(String, Vec<PackagistRelease>)>>;
}

type CandidateCache = HashMap<CandidateCacheKey, CandidateSet>;
type PackageConstraints = HashMap<String, Vec<String>>;
type SelectedPackages = HashMap<String, PackagistRelease>;
pub(crate) type ResolvedPackages = HashMap<String, ResolvedPackageEntry>;

pub(crate) struct ResolveContext<'a> {
    pub repositories: &'a [ComposerRepository],
    pub platform: &'a Platform,
    pub perf: &'a PerfLogger,
    pub reporter: &'a InstallReporter,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct PackageVersionKey {
    package_name: String,
    version: String,
}

struct ResolutionProblem {
    root_requirements: Vec<RequiredPackage>,
    releases: HashMap<PackageVersionKey, PackagistRelease>,
    order: Vec<PackageVersionKey>,
}

pub(crate) trait ResolutionObserver {
    fn package_selected(&mut self, package_name: &str, release: &PackagistRelease);
}

pub(crate) fn resolve_with_observer(
    root_packages: &[RequiredPackage],
    context: &ResolveContext<'_>,
    observer: &mut impl ResolutionObserver,
) -> Result<ResolvedPackages> {
    let package_constraints = root_package_constraints(root_packages);
    let mut candidate_loader = CandidateLoader {
        repositories: context.repositories,
        platform: context.platform,
        perf: context.perf,
        reporter: context.reporter,
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

    context.perf.log(
        "resolve_solver",
        solver_started_at.elapsed(),
        &[
            ("packages", resolved_packages.len().to_string()),
            (
                "candidate_fetches",
                candidate_loader.candidate_cache.len().to_string(),
            ),
            ("versions", stats.versions.to_string()),
            ("dependencies", stats.dependencies.to_string()),
            ("provider_versions", stats.provider_versions.to_string()),
        ],
    )?;
    emit_resolved_package_events(
        &resolved_packages,
        &candidate_loader.candidate_cache,
        context.perf,
        context.reporter,
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
    let root_requirements = requirements_from_constraints(&package_constraints);
    let problem = build_resolution_problem(root_requirements.clone(), candidates, false)?;
    let (constraints, selected) = match solve_pubgrub_problem(&problem, stats) {
        Ok(result) => result,
        Err(_) => {
            let problem = build_resolution_problem(root_requirements, candidates, true)?;

            solve_pubgrub_problem(&problem, stats).map_err(|message| {
                ConcertoError::resolution("dependency graph", &graph_constraints, message)
            })?
        }
    };

    for (package_name, release) in sorted_selected_releases(&selected) {
        observer.package_selected(package_name, release);
    }

    Ok(resolved_package_entries(selected, &constraints))
}

fn requirements_from_constraints(package_constraints: &PackageConstraints) -> Vec<RequiredPackage> {
    let mut requirements = package_constraints
        .iter()
        .flat_map(|(name, constraints)| {
            constraints.iter().map(|constraint| RequiredPackage {
                name: name.clone(),
                constraint: constraint.clone(),
            })
        })
        .collect::<Vec<_>>();
    requirements.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.constraint.cmp(&right.constraint))
    });
    requirements
}

fn build_resolution_problem(
    root_requirements: Vec<RequiredPackage>,
    candidates: &mut impl CandidateProvider,
    discover_all_providers: bool,
) -> Result<ResolutionProblem> {
    let mut releases = HashMap::new();
    let mut order = Vec::new();
    let mut pending = root_requirements.clone();
    let mut processed = HashSet::new();

    while !pending.is_empty() {
        let requirements = std::mem::take(&mut pending);
        let mut prefetch_constraints = PackageConstraints::new();

        for requirement in &requirements {
            if !processed.contains(&requirement_key(requirement)) {
                add_package_constraint(&mut prefetch_constraints, requirement);
            }
        }

        candidates.prefetch(&prefetch_constraints, &SelectedPackages::new())?;

        for requirement in requirements {
            if !processed.insert(requirement_key(&requirement)) {
                continue;
            }

            if is_package_name(&requirement.name) {
                let direct_releases = matching_releases(
                    &requirement.name,
                    std::slice::from_ref(&requirement.constraint),
                    candidates,
                );

                let direct_releases = match direct_releases {
                    Ok(releases) => releases,
                    Err(error) => {
                        let provider_release_count = queue_capability_provider_releases(
                            &requirement,
                            candidates,
                            &mut releases,
                            &mut order,
                            &mut pending,
                        )?;

                        if provider_release_count == 0 {
                            return Err(error);
                        }

                        continue;
                    }
                };
                for release in direct_releases {
                    queue_release(
                        &requirement.name,
                        release,
                        &mut releases,
                        &mut order,
                        &mut pending,
                    );
                }
                if discover_all_providers {
                    queue_capability_provider_releases(
                        &requirement,
                        candidates,
                        &mut releases,
                        &mut order,
                        &mut pending,
                    )?;
                }
            } else {
                queue_capability_provider_releases(
                    &requirement,
                    candidates,
                    &mut releases,
                    &mut order,
                    &mut pending,
                )?;
            }
        }
    }

    Ok(ResolutionProblem {
        root_requirements,
        releases,
        order,
    })
}

fn requirement_key(requirement: &RequiredPackage) -> (String, String) {
    (requirement.name.clone(), requirement.constraint.clone())
}

fn solve_pubgrub_problem(
    problem: &ResolutionProblem,
    stats: &mut SolverStats,
) -> std::result::Result<(PackageConstraints, SelectedPackages), String> {
    let provider = PubGrubProvider::new(problem).map_err(|error| error.to_string())?;
    stats.versions = provider.version_count();
    stats.dependencies = provider.dependency_count();
    stats.provider_versions = provider.provider_version_count();

    let solution =
        resolve(&provider, PubGrubPackage::Root, PubGrubVersion::Root).map_err(pubgrub_error)?;
    let selected = selected_from_pubgrub_solution(problem, solution);
    let constraints =
        selected_constraints(problem, &selected).map_err(|error| error.to_string())?;

    Ok((constraints, selected))
}

fn queue_capability_provider_releases(
    requirement: &RequiredPackage,
    candidates: &mut impl CandidateProvider,
    releases: &mut HashMap<PackageVersionKey, PackagistRelease>,
    order: &mut Vec<PackageVersionKey>,
    pending: &mut Vec<RequiredPackage>,
) -> Result<usize> {
    let mut release_count = 0;

    for (provider_name, provider_releases) in candidates.load_capability_providers(requirement)? {
        release_count += provider_releases.len();

        for release in provider_releases {
            queue_release(&provider_name, release, releases, order, pending);
        }
    }

    Ok(release_count)
}

fn queue_release(
    package_name: &str,
    release: PackagistRelease,
    releases: &mut HashMap<PackageVersionKey, PackagistRelease>,
    order: &mut Vec<PackageVersionKey>,
    pending: &mut Vec<RequiredPackage>,
) {
    let key = PackageVersionKey {
        package_name: package_name.to_string(),
        version: release.version.clone(),
    };

    if releases.insert(key.clone(), release.clone()).is_none() {
        order.push(key);
        pending.extend(release.package_requires);
    }
}

fn release_satisfies_requirement(
    key: &PackageVersionKey,
    release: &PackagistRelease,
    requirement: &RequiredPackage,
    constraints: &[String],
) -> Result<bool> {
    if key.package_name == requirement.name
        && release.matches_constraints(&requirement.name, constraints)?
    {
        return Ok(true);
    }

    for capability in release.provides.iter().chain(&release.replaces) {
        if capability.name == requirement.name
            && capability_constraint_matches(
                &capability.constraint,
                &release.version,
                &requirement.constraint,
            )?
        {
            return Ok(true);
        }
    }

    Ok(false)
}

fn capability_constraint_matches(
    provided_constraint: &str,
    provider_version: &str,
    required_constraint: &str,
) -> Result<bool> {
    if provided_constraint == "*" || required_constraint == "*" {
        return Ok(true);
    }

    let provided_version = if provided_constraint == "self.version" {
        provider_version
    } else {
        provided_constraint
    };

    match semver_php::Semver::satisfies(provided_version, required_constraint) {
        Ok(matches) => Ok(matches),
        Err(_) => Ok(provided_constraint == required_constraint),
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum PubGrubPackage {
    Root,
    Package(String),
}

impl fmt::Display for PubGrubPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => formatter.write_str("root"),
            Self::Package(package) => formatter.write_str(package),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum PubGrubVersion {
    Root,
    Absent,
    Release(String),
    Provided { package: String, version: String },
}

impl fmt::Display for PubGrubVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => formatter.write_str("1.0.0"),
            Self::Absent => formatter.write_str("absent"),
            Self::Release(version) => formatter.write_str(version),
            Self::Provided { package, version } => write!(formatter, "{package}@{version}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DiscreteVersionSet {
    Any,
    Only(BTreeSet<PubGrubVersion>),
    Except(BTreeSet<PubGrubVersion>),
}

impl DiscreteVersionSet {
    fn only(versions: impl IntoIterator<Item = PubGrubVersion>) -> Self {
        Self::Only(versions.into_iter().collect())
    }

    fn without(versions: impl IntoIterator<Item = PubGrubVersion>) -> Self {
        let versions = versions.into_iter().collect::<BTreeSet<_>>();

        if versions.is_empty() {
            Self::Any
        } else {
            Self::Except(versions)
        }
    }
}

impl fmt::Display for DiscreteVersionSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => formatter.write_str("*"),
            Self::Only(versions) if versions.is_empty() => formatter.write_str("<empty>"),
            Self::Only(versions) => write!(formatter, "{}", display_versions(versions)),
            Self::Except(versions) => write!(formatter, "not {}", display_versions(versions)),
        }
    }
}

fn display_versions(versions: &BTreeSet<PubGrubVersion>) -> String {
    versions
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" || ")
}

impl VersionSet for DiscreteVersionSet {
    type V = PubGrubVersion;

    fn empty() -> Self {
        Self::Only(BTreeSet::new())
    }

    fn singleton(version: Self::V) -> Self {
        Self::Only(BTreeSet::from([version]))
    }

    fn complement(&self) -> Self {
        match self {
            Self::Any => Self::empty(),
            Self::Only(versions) => Self::without(versions.clone()),
            Self::Except(versions) => Self::Only(versions.clone()),
        }
    }

    fn intersection(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Any, right) => right.clone(),
            (left, Self::Any) => left.clone(),
            (Self::Only(left), Self::Only(right)) => {
                Self::Only(left.intersection(right).cloned().collect())
            }
            (Self::Only(left), Self::Except(right)) | (Self::Except(right), Self::Only(left)) => {
                Self::Only(left.difference(right).cloned().collect())
            }
            (Self::Except(left), Self::Except(right)) => Self::without(left.union(right).cloned()),
        }
    }

    fn contains(&self, version: &Self::V) -> bool {
        match self {
            Self::Any => true,
            Self::Only(versions) => versions.contains(version),
            Self::Except(versions) => !versions.contains(version),
        }
    }
}

struct PubGrubProvider<'a> {
    versions: HashMap<PubGrubPackage, Vec<PubGrubVersion>>,
    dependencies:
        HashMap<(PubGrubPackage, PubGrubVersion), Vec<(PubGrubPackage, DiscreteVersionSet)>>,
    problem: &'a ResolutionProblem,
}

impl<'a> PubGrubProvider<'a> {
    fn new(problem: &'a ResolutionProblem) -> Result<Self> {
        let mut provider = Self {
            versions: HashMap::new(),
            dependencies: HashMap::new(),
            problem,
        };

        provider.add_root_dependencies()?;
        provider.add_release_dependencies()?;
        provider.add_absent_versions();

        Ok(provider)
    }

    fn version_count(&self) -> usize {
        self.versions.values().map(Vec::len).sum()
    }

    fn dependency_count(&self) -> usize {
        self.dependencies.values().map(Vec::len).sum()
    }

    fn provider_version_count(&self) -> usize {
        self.versions
            .values()
            .flatten()
            .filter(|version| matches!(version, PubGrubVersion::Provided { .. }))
            .count()
    }

    fn add_root_dependencies(&mut self) -> Result<()> {
        let dependencies = self
            .problem
            .root_requirements
            .iter()
            .map(|requirement| self.dependency(requirement))
            .collect::<Result<Vec<_>>>()?;

        self.versions
            .entry(PubGrubPackage::Root)
            .or_default()
            .push(PubGrubVersion::Root);
        self.dependencies
            .insert((PubGrubPackage::Root, PubGrubVersion::Root), dependencies);

        Ok(())
    }

    fn add_release_dependencies(&mut self) -> Result<()> {
        for key in &self.problem.order {
            let package = PubGrubPackage::Package(key.package_name.clone());
            let version = PubGrubVersion::Release(key.version.clone());
            let release = &self.problem.releases[key];
            let mut dependencies = Vec::new();

            self.versions
                .entry(package.clone())
                .or_default()
                .push(version.clone());

            for requirement in &release.package_requires {
                dependencies.push(self.dependency(requirement)?);
            }

            for conflict in &release.conflicts {
                self.versions
                    .entry(PubGrubPackage::Package(conflict.name.clone()))
                    .or_default();
                dependencies.push((
                    PubGrubPackage::Package(conflict.name.clone()),
                    DiscreteVersionSet::without(self.matching_versions(conflict)?),
                ));
            }

            for replacement in &release.replaces {
                self.versions
                    .entry(PubGrubPackage::Package(replacement.name.clone()))
                    .or_default();
                dependencies.push((
                    PubGrubPackage::Package(replacement.name.clone()),
                    DiscreteVersionSet::without(self.matching_real_versions(replacement)?),
                ));
            }

            self.dependencies
                .insert((package.clone(), version.clone()), dependencies);
            self.add_capabilities(key, release);
        }

        Ok(())
    }

    fn add_absent_versions(&mut self) {
        let packages = self.versions.keys().cloned().collect::<Vec<_>>();

        for package in packages {
            if package == PubGrubPackage::Root {
                continue;
            }

            self.versions
                .entry(package.clone())
                .or_default()
                .push(PubGrubVersion::Absent);
            self.dependencies
                .insert((package, PubGrubVersion::Absent), Vec::new());
        }
    }

    fn add_capabilities(&mut self, key: &PackageVersionKey, release: &PackagistRelease) {
        for capability in release.provides.iter().chain(&release.replaces) {
            let package = PubGrubPackage::Package(capability.name.clone());
            let version = PubGrubVersion::Provided {
                package: key.package_name.clone(),
                version: key.version.clone(),
            };

            self.versions
                .entry(package.clone())
                .or_default()
                .push(version.clone());
            self.dependencies.insert(
                (package, version),
                vec![(
                    PubGrubPackage::Package(key.package_name.clone()),
                    DiscreteVersionSet::singleton(PubGrubVersion::Release(key.version.clone())),
                )],
            );
        }
    }

    fn dependency(
        &self,
        requirement: &RequiredPackage,
    ) -> Result<(PubGrubPackage, DiscreteVersionSet)> {
        Ok((
            PubGrubPackage::Package(requirement.name.clone()),
            DiscreteVersionSet::only(self.matching_versions(requirement)?),
        ))
    }

    fn matching_versions(&self, requirement: &RequiredPackage) -> Result<Vec<PubGrubVersion>> {
        let mut versions = self.matching_real_versions(requirement)?;

        for key in &self.problem.order {
            let release = &self.problem.releases[key];

            for capability in release.provides.iter().chain(&release.replaces) {
                if capability.name == requirement.name
                    && capability_constraint_matches(
                        &capability.constraint,
                        &release.version,
                        &requirement.constraint,
                    )?
                {
                    versions.push(PubGrubVersion::Provided {
                        package: key.package_name.clone(),
                        version: key.version.clone(),
                    });
                }
            }
        }

        Ok(versions)
    }

    fn matching_real_versions(&self, requirement: &RequiredPackage) -> Result<Vec<PubGrubVersion>> {
        let constraints = std::slice::from_ref(&requirement.constraint);
        let mut versions = Vec::new();

        for key in &self.problem.order {
            if key.package_name != requirement.name {
                continue;
            }

            let release = &self.problem.releases[key];
            if release.matches_constraints(&requirement.name, constraints)? {
                versions.push(PubGrubVersion::Release(key.version.clone()));
            }
        }

        Ok(versions)
    }
}

impl DependencyProvider for PubGrubProvider<'_> {
    type Err = Infallible;
    type M = String;
    type P = PubGrubPackage;
    type Priority = (u32, Reverse<usize>);
    type V = PubGrubVersion;
    type VS = DiscreteVersionSet;

    fn prioritize(
        &self,
        package: &Self::P,
        range: &Self::VS,
        stats: &PackageResolutionStatistics,
    ) -> Self::Priority {
        let version_count = self
            .versions
            .get(package)
            .map(|versions| {
                versions
                    .iter()
                    .filter(|version| range.contains(version))
                    .count()
            })
            .unwrap_or_default();

        if version_count == 0 {
            return (u32::MAX, Reverse(0));
        }

        (stats.conflict_count(), Reverse(version_count))
    }

    fn choose_version(
        &self,
        package: &Self::P,
        range: &Self::VS,
    ) -> std::result::Result<Option<Self::V>, Self::Err> {
        Ok(self.versions.get(package).and_then(|versions| {
            versions
                .iter()
                .filter(|version| range.contains(version))
                .find(|version| !matches!(version, PubGrubVersion::Absent))
                .or_else(|| versions.iter().find(|version| range.contains(version)))
                .cloned()
        }))
    }

    fn get_dependencies(
        &self,
        package: &Self::P,
        version: &Self::V,
    ) -> std::result::Result<Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
        Ok(self
            .dependencies
            .get(&(package.clone(), version.clone()))
            .cloned()
            .map(DependencyConstraints::from_iter)
            .map(Dependencies::Available)
            .unwrap_or_else(|| Dependencies::Unavailable("metadata unavailable".to_string())))
    }
}

fn pubgrub_error(error: PubGrubError<PubGrubProvider<'_>>) -> String {
    match error {
        PubGrubError::NoSolution(mut tree) => {
            tree.collapse_no_versions();
            DefaultStringReporter::report(&tree)
        }
        other => format!("{other:?}"),
    }
}

fn selected_from_pubgrub_solution(
    problem: &ResolutionProblem,
    solution: pubgrub::SelectedDependencies<PubGrubPackage, PubGrubVersion>,
) -> SelectedPackages {
    let mut selected = SelectedPackages::new();

    for (package, version) in solution {
        let PubGrubPackage::Package(package_name) = package else {
            continue;
        };
        let PubGrubVersion::Release(version) = version else {
            continue;
        };
        let key = PackageVersionKey {
            package_name: package_name.clone(),
            version,
        };

        if let Some(release) = problem.releases.get(&key) {
            selected.insert(package_name, release.clone());
        }
    }

    selected
}

fn selected_constraints(
    problem: &ResolutionProblem,
    selected: &SelectedPackages,
) -> Result<PackageConstraints> {
    let mut constraints = PackageConstraints::new();
    let mut pending = problem.root_requirements.clone();
    let mut processed = HashSet::new();

    while let Some(requirement) = pending.pop() {
        add_package_constraint(&mut constraints, &requirement);

        if !processed.insert(requirement_key(&requirement)) {
            continue;
        }

        for (package_name, release) in selected {
            let key = PackageVersionKey {
                package_name: package_name.clone(),
                version: release.version.clone(),
            };

            if release_satisfies_requirement(
                &key,
                release,
                &requirement,
                std::slice::from_ref(&requirement.constraint),
            )? {
                pending.extend(release.package_requires.clone());
            }
        }
    }

    Ok(constraints)
}

fn sorted_selected_releases(selected: &SelectedPackages) -> Vec<(&str, &PackagistRelease)> {
    let mut releases = selected
        .iter()
        .map(|(package_name, release)| (package_name.as_str(), release))
        .collect::<Vec<_>>();
    releases.sort_by(|left, right| left.0.cmp(right.0));
    releases
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
                    provides: release.provides,
                    replaces: release.replaces,
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
            match self.fetch_candidate_batch(batch) {
                Ok(fetches) => {
                    for fetch in fetches {
                        self.record(fetch)?;
                    }
                }
                Err(_) => {
                    for job in batch {
                        if let Ok(fetch) = self.fetch(job.key.clone()) {
                            self.record(fetch)?;
                        }
                    }
                }
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

    fn load_capability_providers(
        &mut self,
        requirement: &RequiredPackage,
    ) -> Result<Vec<(String, Vec<PackagistRelease>)>> {
        let started_at = Instant::now();
        let constraints = std::slice::from_ref(&requirement.constraint);
        let metadata = provider_metadata(&requirement.name, constraints, self.repositories)?;
        let provider_names = packagist::provider_names(&metadata, &requirement.name, constraints)?;

        self.perf.log(
            "resolve_providers",
            started_at.elapsed(),
            &[
                ("package", requirement.name.clone()),
                ("bytes", metadata.len().to_string()),
                ("providers", provider_names.len().to_string()),
                ("constraints", requirement.constraint.clone()),
            ],
        )?;

        let empty_constraints = Vec::new();
        let mut providers = Vec::new();

        for batch in provider_names.chunks(MAX_PARALLEL_CANDIDATE_FETCHES) {
            let jobs = batch
                .iter()
                .map(|package_name| CandidateFetchJob {
                    key: candidate_cache_key(package_name, &empty_constraints),
                })
                .filter(|job| !self.candidate_cache.contains_key(&job.key))
                .collect::<Vec<_>>();

            match self.fetch_candidate_batch(&jobs) {
                Ok(fetches) => {
                    for fetch in fetches {
                        self.record(fetch)?;
                    }
                }
                Err(_) => {
                    for job in &jobs {
                        if let Ok(fetch) = self.fetch(job.key.clone()) {
                            self.record(fetch)?;
                        }
                    }
                }
            }

            for provider_name in batch {
                let mut releases = Vec::new();
                let Ok(provider_releases) = self.load(provider_name, &empty_constraints) else {
                    continue;
                };

                for release in provider_releases {
                    let key = PackageVersionKey {
                        package_name: provider_name.clone(),
                        version: release.version.clone(),
                    };

                    if release_satisfies_requirement(&key, &release, requirement, constraints)? {
                        releases.push(release);
                    }
                }

                if !releases.is_empty() {
                    providers.push((provider_name.clone(), releases));
                }
            }
        }

        Ok(providers)
    }
}

impl CandidateLoader<'_> {
    fn fetch(&self, key: CandidateCacheKey) -> Result<CandidateFetch> {
        if let Some(metadata) = self.metadata_cache.get(&key.package_name) {
            return parse_candidate_metadata(key, metadata, self.platform, None, Instant::now());
        }

        fetch_candidate(key, self.repositories, self.platform)
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

        fetched.append(&mut fetch_candidate_batch(
            &network_jobs,
            self.repositories,
            self.platform,
        )?);

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
    repositories: &[ComposerRepository],
    platform: &Platform,
) -> Result<Vec<CandidateFetch>> {
    std::thread::scope(|scope| {
        let handles = jobs
            .iter()
            .map(|job| {
                scope.spawn(move || fetch_candidate(job.key.clone(), repositories, platform))
            })
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

fn fetch_candidate(
    key: CandidateCacheKey,
    repositories: &[ComposerRepository],
    platform: &Platform,
) -> Result<CandidateFetch> {
    let started_at = Instant::now();
    let metadata = package_metadata(&key.package_name, &key.constraints, repositories)?;
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

fn package_metadata(
    package_name: &str,
    constraints: &[String],
    repositories: &[ComposerRepository],
) -> Result<String> {
    for repository in repositories {
        let metadata_url = packagist::repository_package_url(&repository.url, package_name)?;

        if let Ok(metadata) = get_text(&metadata_url) {
            return Ok(metadata);
        }
    }

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

fn provider_metadata(
    package_name: &str,
    constraints: &[String],
    repositories: &[ComposerRepository],
) -> Result<String> {
    for repository in repositories {
        let metadata_url = packagist::repository_providers_url(&repository.url, package_name)?;

        if let Ok(metadata) = get_text(&metadata_url) {
            return Ok(metadata);
        }
    }

    if let Ok(fixtures_dir) = std::env::var(PACKAGIST_FIXTURES_DIR_ENV) {
        let path = fixture_provider_metadata_path(Path::new(&fixtures_dir), package_name);

        return match std::fs::read_to_string(&path) {
            Ok(content) => Ok(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(r#"{"providers":[]}"#.to_string())
            }
            Err(error) => Err(ConcertoError::resolution(
                package_name,
                constraints,
                format!(
                    "could not read Packagist provider fixture at {}: {}",
                    path.display(),
                    error
                ),
            )),
        };
    }

    let metadata_url = packagist::providers_url(package_name)?;
    let metadata = get_text(&metadata_url).map_err(|error| {
        ConcertoError::resolution(
            package_name,
            constraints,
            format!("could not fetch Packagist provider metadata from {metadata_url}: {error}"),
        )
    })?;

    Ok(metadata)
}

fn fixture_metadata_path(fixtures_dir: &Path, package_name: &str) -> PathBuf {
    fixtures_dir.join(format!("{}.json", package_name.replace('/', "-")))
}

fn fixture_provider_metadata_path(fixtures_dir: &Path, package_name: &str) -> PathBuf {
    fixtures_dir.join(format!("providers-{}.json", package_name.replace('/', "-")))
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
        assert!(stats.versions > 0);
        assert!(stats.dependencies > 0);
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
        assert!(error.contains("root 1.0.0 is forbidden"));
    }

    #[test]
    fn conflict_rejects_candidate_that_blocks_selected_package() {
        let mut stats = SolverStats::default();
        let mut candidates = InMemoryCandidates::new([
            (
                "acme/app",
                vec![
                    release_with_links(
                        "1.1.0",
                        &[],
                        &[required_package("acme/broken", "<2.0")],
                        &[],
                        &[],
                    ),
                    release("1.0.0", &[]),
                ],
            ),
            ("acme/broken", vec![release("1.0.0", &[])]),
        ]);

        let resolved = solve_dependency_graph(
            root_package_constraints(&[
                required_package("acme/app", "^1.0"),
                required_package("acme/broken", "^1.0"),
            ]),
            &mut candidates,
            &mut stats,
            &mut InMemoryObserver,
        )
        .unwrap();

        assert_eq!(resolved["acme/app"].version, "1.0.0");
        assert_eq!(resolved["acme/broken"].version, "1.0.0");
    }

    #[test]
    fn provide_satisfies_virtual_dependency() {
        let mut stats = SolverStats::default();
        let mut candidates = InMemoryCandidates::new([
            (
                "acme/app",
                vec![release(
                    "1.0.0",
                    &[required_package("psr/log-implementation", "^1.0")],
                )],
            ),
            (
                "acme/logger",
                vec![release_with_links(
                    "1.0.0",
                    &[],
                    &[],
                    &[required_package("psr/log-implementation", "1.0.0")],
                    &[],
                )],
            ),
        ]);

        let resolved = solve_dependency_graph(
            root_package_constraints(&[
                required_package("acme/app", "^1.0"),
                required_package("acme/logger", "^1.0"),
            ]),
            &mut candidates,
            &mut stats,
            &mut InMemoryObserver,
        )
        .unwrap();

        assert_eq!(resolved["acme/app"].version, "1.0.0");
        assert_eq!(resolved["acme/logger"].version, "1.0.0");
        assert!(!resolved.contains_key("psr/log-implementation"));
    }

    #[test]
    fn replace_satisfies_replaced_package_dependency() {
        let mut stats = SolverStats::default();
        let mut candidates = InMemoryCandidates::new([
            (
                "acme/app",
                vec![release(
                    "1.0.0",
                    &[required_package("acme/old-logger", "^1.0")],
                )],
            ),
            (
                "acme/logger",
                vec![release_with_links(
                    "1.0.0",
                    &[],
                    &[],
                    &[],
                    &[required_package("acme/old-logger", "self.version")],
                )],
            ),
        ]);

        let resolved = solve_dependency_graph(
            root_package_constraints(&[
                required_package("acme/app", "^1.0"),
                required_package("acme/logger", "^1.0"),
            ]),
            &mut candidates,
            &mut stats,
            &mut InMemoryObserver,
        )
        .unwrap();

        assert_eq!(resolved["acme/app"].version, "1.0.0");
        assert_eq!(resolved["acme/logger"].version, "1.0.0");
        assert!(!resolved.contains_key("acme/old-logger"));
    }

    #[test]
    fn discovers_provider_for_virtual_root_requirement() {
        let mut stats = SolverStats::default();
        let mut candidates = InMemoryCandidates::new([(
            "acme/logger",
            vec![release_with_links(
                "1.0.0",
                &[],
                &[],
                &[required_package("psr/log-implementation", "1.0.0")],
                &[],
            )],
        )]);

        let resolved = solve_dependency_graph(
            root_package_constraints(&[required_package("psr/log-implementation", "^1.0")]),
            &mut candidates,
            &mut stats,
            &mut InMemoryObserver,
        )
        .unwrap();

        assert_eq!(resolved["acme/logger"].version, "1.0.0");
        assert!(!resolved.contains_key("psr/log-implementation"));
    }

    #[test]
    fn considers_provider_even_when_direct_package_exists() {
        let mut stats = SolverStats::default();
        let mut candidates = InMemoryCandidates::new([
            (
                "acme/contract",
                vec![release(
                    "1.0.0",
                    &[required_package("acme/implementation", "^1.0")],
                )],
            ),
            ("acme/implementation", vec![release("2.0.0", &[])]),
            (
                "acme/provider",
                vec![release_with_links(
                    "1.0.0",
                    &[],
                    &[],
                    &[required_package("acme/contract", "1.0.0")],
                    &[],
                )],
            ),
        ]);

        let resolved = solve_dependency_graph(
            root_package_constraints(&[
                required_package("acme/contract", "^1.0"),
                required_package("acme/implementation", "^2.0"),
            ]),
            &mut candidates,
            &mut stats,
            &mut InMemoryObserver,
        )
        .unwrap();

        assert_eq!(resolved["acme/provider"].version, "1.0.0");
        assert_eq!(resolved["acme/implementation"].version, "2.0.0");
        assert!(!resolved.contains_key("acme/contract"));
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

        fn load_capability_providers(
            &mut self,
            requirement: &RequiredPackage,
        ) -> Result<Vec<(String, Vec<PackagistRelease>)>> {
            let constraints = std::slice::from_ref(&requirement.constraint);
            let mut providers = Vec::new();

            for (package_name, releases) in &self.packages {
                let mut matching_releases = Vec::new();

                for release in releases {
                    let key = PackageVersionKey {
                        package_name: package_name.clone(),
                        version: release.version.clone(),
                    };

                    if release_satisfies_requirement(&key, release, requirement, constraints)? {
                        matching_releases.push(release.clone());
                    }
                }

                if !matching_releases.is_empty() {
                    providers.push((package_name.clone(), matching_releases));
                }
            }

            providers.sort_by(|left, right| left.0.cmp(&right.0));

            Ok(providers)
        }
    }

    fn release(version: &str, package_requires: &[RequiredPackage]) -> PackagistRelease {
        release_with_links(version, package_requires, &[], &[], &[])
    }

    fn release_with_links(
        version: &str,
        package_requires: &[RequiredPackage],
        conflicts: &[RequiredPackage],
        provides: &[RequiredPackage],
        replaces: &[RequiredPackage],
    ) -> PackagistRelease {
        PackagistRelease {
            version_count: 2,
            version: version.to_string(),
            dist_url: format!("https://example.com/{version}.zip"),
            package_requires: package_requires.to_vec(),
            platform_requires: Vec::new(),
            conflicts: conflicts.to_vec(),
            provides: provides.to_vec(),
            replaces: replaces.to_vec(),
        }
    }

    fn required_package(name: &str, constraint: &str) -> RequiredPackage {
        RequiredPackage {
            name: name.to_string(),
            constraint: constraint.to_string(),
        }
    }
}
