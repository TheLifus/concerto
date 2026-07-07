use crate::composer::{ComposerRepository, RequiredPackage};
use crate::error::{ConcertoError, Result};

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct Lockfile {
    pub lockfile_version: u8,
    pub root_manifest_hash: String,
    pub root_requirements: Vec<RequiredPackage>,
    #[serde(default)]
    pub root_repositories: Vec<ComposerRepository>,
    pub packages: Vec<LockedPackage>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct LockedPackage {
    pub name: String,
    pub version: String,
    pub dist_url: String,
    #[serde(default)]
    pub dev: bool,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

pub(crate) const LOCKFILE_VERSION: u8 = 2;
pub(crate) const LOCKFILE_PATH: &str = "concerto.lock";

pub(crate) fn write(lockfile: &Lockfile) -> Result<()> {
    let content = serde_json::to_string_pretty(lockfile).map_err(|error| {
        ConcertoError::lockfile(format!("Could not serialize lockfile: {error}"))
    })?;

    std::fs::write(LOCKFILE_PATH, content)
        .map_err(|error| ConcertoError::lockfile(format!("Could not write lockfile: {error}")))
}

pub(crate) fn read() -> Result<Option<Lockfile>> {
    match std::fs::read_to_string(LOCKFILE_PATH) {
        Ok(content) => parse(&content).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConcertoError::lockfile(format!(
            "Could not read lockfile: {error}"
        ))),
    }
}

fn parse(content: &str) -> Result<Lockfile> {
    let lockfile: Lockfile = serde_json::from_str(content)
        .map_err(|error| ConcertoError::lockfile(format!("Invalid lockfile: {error}")))?;

    if lockfile.lockfile_version != LOCKFILE_VERSION {
        return Err(ConcertoError::lockfile(format!(
            "Unsupported lockfile version: {}",
            lockfile.lockfile_version
        )));
    }

    if lockfile.root_manifest_hash
        != root_manifest_hash(&lockfile.root_requirements, &lockfile.root_repositories)
    {
        return Err(ConcertoError::lockfile(
            "Lockfile root manifest hash does not match root requirements and repositories",
        ));
    }

    Ok(lockfile)
}

pub(crate) fn matches_root_manifest(
    lockfile: &Lockfile,
    root_requirements: &[RequiredPackage],
    root_repositories: &[ComposerRepository],
) -> bool {
    lockfile.root_manifest_hash == root_manifest_hash(root_requirements, root_repositories)
}

pub(crate) fn root_manifest_hash(
    root_requirements: &[RequiredPackage],
    root_repositories: &[ComposerRepository],
) -> String {
    let mut requirements = root_requirements.to_vec();
    requirements.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.constraint.cmp(&right.constraint))
    });

    let mut hasher = blake3::Hasher::new();

    hash_value(&mut hasher, "requirements");
    for requirement in requirements {
        hash_value(&mut hasher, &requirement.name);
        hash_value(&mut hasher, &requirement.constraint);
    }
    hash_value(&mut hasher, "repositories");
    for repository in root_repositories {
        hash_value(&mut hasher, &repository.url);
    }

    hasher.finalize().to_hex().to_string()
}

fn hash_value(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests;
