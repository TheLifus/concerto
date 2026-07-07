use crate::composer::package_path_parts;
use crate::error::{ConcertoError, Result, StoreStep};
use std::path::PathBuf;

pub(super) struct PackageBasePaths {
    package_name: String,
    pub(super) vendor_link: PathBuf,
    pub(super) version_store: PathBuf,
    pub(super) download_zip: PathBuf,
}

pub(super) struct PackagePaths {
    package_name: String,
    pub(super) zip: PathBuf,
    pub(super) integrity: PathBuf,
    pub(super) content_store: PathBuf,
    pub(super) published_source: PathBuf,
    pub(super) staged_source: PathBuf,
    pub(super) content_integrity: String,
}

pub(super) fn package_base_paths(package_name: &str, version: &str) -> Result<PackageBasePaths> {
    let (vendor, name) = package_path_parts(package_name).map_err(|error| {
        ConcertoError::store(package_name, StoreStep::Prepare, error.to_string())
    })?;
    let version_store = PathBuf::from(".concerto/store")
        .join(vendor)
        .join(name)
        .join(version);
    let vendor_parent = PathBuf::from("vendor").join(vendor);
    let vendor_link = vendor_parent.join(name);

    Ok(PackageBasePaths {
        package_name: package_name.to_string(),
        vendor_link,
        download_zip: version_store.join("package.zip.tmp"),
        version_store,
    })
}

pub(super) fn package_paths(
    base_paths: &PackageBasePaths,
    integrity: &str,
) -> Result<PackagePaths> {
    let content_store = base_paths
        .version_store
        .join(integrity_store_key(base_paths.package_name(), integrity)?);

    Ok(PackagePaths {
        package_name: base_paths.package_name.clone(),
        zip: content_store.join("package.zip"),
        integrity: content_store.join("integrity"),
        published_source: content_store.join("source"),
        staged_source: content_store.join("source.tmp"),
        content_store,
        content_integrity: integrity.to_string(),
    })
}

impl PackageBasePaths {
    pub(super) fn package_name(&self) -> &str {
        &self.package_name
    }
}

impl PackagePaths {
    pub(super) fn package_name(&self) -> &str {
        &self.package_name
    }
}

fn integrity_store_key(package_name: &str, integrity: &str) -> Result<String> {
    let Some(hash) = integrity.strip_prefix("blake3:") else {
        return Err(invalid_integrity(package_name, integrity));
    };

    if hash.len() != 64 || !hash.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(invalid_integrity(package_name, integrity));
    }

    Ok(format!("blake3-{hash}"))
}

fn invalid_integrity(package_name: &str, integrity: &str) -> ConcertoError {
    ConcertoError::store(
        package_name,
        StoreStep::Prepare,
        format!("invalid archive integrity: {integrity}"),
    )
}
