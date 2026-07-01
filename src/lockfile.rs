use crate::composer::RequiredPackage;

#[derive(serde::Serialize)]
pub(crate) struct Lockfile {
    pub packages: Vec<LockedPackage>,
}

#[derive(serde::Serialize)]
pub(crate) struct LockedPackage {
    pub name: String,
    pub version: String,
    pub dist_url: String,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

pub(crate) const LOCKFILE_PATH: &str = "concerto.lock";

pub(crate) fn write(lockfile: &Lockfile) -> Result<(), String> {
    let content = serde_json::to_string_pretty(lockfile)
        .map_err(|error| format!("Could not serialize lockfile: {error}"))?;

    std::fs::write(LOCKFILE_PATH, content)
        .map_err(|error| format!("Could not write lockfile: {error}"))
}
