use crate::composer::RequiredPackage;
use crate::error::{ConcertoError, Result};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct Lockfile {
    pub lockfile_version: u8,
    pub root_requirements_hash: String,
    pub root_requirements: Vec<RequiredPackage>,
    pub packages: Vec<LockedPackage>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct LockedPackage {
    pub name: String,
    pub version: String,
    pub dist_url: String,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

pub(crate) const LOCKFILE_VERSION: u8 = 1;
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

    if lockfile.root_requirements_hash != root_requirements_hash(&lockfile.root_requirements) {
        return Err(ConcertoError::lockfile(
            "Lockfile root requirements hash does not match requirements",
        ));
    }

    Ok(lockfile)
}

pub(crate) fn matches_root_requirements(
    lockfile: &Lockfile,
    root_requirements: &[RequiredPackage],
) -> bool {
    lockfile.root_requirements_hash == root_requirements_hash(root_requirements)
}

pub(crate) fn root_requirements_hash(root_requirements: &[RequiredPackage]) -> String {
    let mut requirements = root_requirements.to_vec();
    requirements.sort_by(|left, right| left.name.cmp(&right.name));

    let mut hasher = blake3::Hasher::new();

    for requirement in requirements {
        hash_value(&mut hasher, &requirement.name);
        hash_value(&mut hasher, &requirement.constraint);
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
