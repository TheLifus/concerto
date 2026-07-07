mod archive;
mod paths;

use crate::error::{ConcertoError, Result, StoreStep};
use crate::http::download_to_file;
pub(crate) use archive::{IntegrityCheck, IntegrityCheckKind};
use archive::{
    verify_downloaded_archive, verify_stored_integrity, verify_unsafe_trusted_store_marker,
    write_integrity,
};
use paths::{PackageBasePaths, PackagePaths, package_base_paths, package_paths};
use std::path::{Path, PathBuf};

pub(crate) struct PackageSource {
    package_name: String,
    path: PathBuf,
    vendor_link: PathBuf,
    reused: bool,
    integrity: String,
    integrity_check: Option<IntegrityCheck>,
}

pub(crate) struct PackageArchive<'a> {
    pub version: &'a str,
    pub dist_url: &'a str,
    pub expected_integrity: Option<&'a str>,
    pub expected_shasum: Option<&'a str>,
    pub unsafe_trust_store: bool,
}

impl PackageSource {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn is_reused(&self) -> bool {
        self.reused
    }

    pub(crate) fn integrity(&self) -> &str {
        &self.integrity
    }

    pub(crate) fn integrity_check(&self) -> Option<IntegrityCheck> {
        self.integrity_check
    }
}

pub(crate) fn prepare_source(
    package_name: &str,
    archive: PackageArchive<'_>,
) -> Result<PackageSource> {
    let base_paths = package_base_paths(package_name, archive.version)?;

    let (path, integrity, reused, integrity_check) = match archive.expected_integrity {
        Some(expected_integrity) => {
            let paths = package_paths(&base_paths, expected_integrity)?;
            match existing_source(&paths, archive.unsafe_trust_store)? {
                Some((source, check)) => {
                    (source, expected_integrity.to_string(), true, Some(check))
                }
                None => {
                    let (source, integrity, check) =
                        download_and_publish_source(archive, &base_paths)?;

                    (source, integrity, false, Some(check))
                }
            }
        }
        None => {
            let (source, integrity, check) = download_and_publish_source(archive, &base_paths)?;

            (source, integrity, false, Some(check))
        }
    };

    Ok(PackageSource {
        package_name: package_name.to_string(),
        path,
        vendor_link: base_paths.vendor_link,
        reused,
        integrity,
        integrity_check,
    })
}

pub(crate) fn link_to_vendor(source: &PackageSource) -> Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(&source.vendor_link) {
        if !metadata.file_type().is_symlink() {
            return Err(ConcertoError::store_with_hint(
                &source.package_name,
                StoreStep::Link,
                format!(
                    "vendor path already exists and is not a symlink: {}",
                    source.vendor_link.display()
                ),
                "Remove or move the existing vendor path, then run install again.",
            ));
        }

        if std::fs::read_link(&source.vendor_link).is_ok_and(|target| target == source.path) {
            return Ok(());
        }

        std::fs::remove_file(&source.vendor_link).map_err(|error| {
            ConcertoError::store(
                &source.package_name,
                StoreStep::Link,
                format!("could not remove existing vendor link: {error}"),
            )
        })?;
    }

    if let Some(parent) = source.vendor_link.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ConcertoError::store(
                &source.package_name,
                StoreStep::Link,
                format!("could not create vendor directory: {error}"),
            )
        })?;
    }

    std::os::unix::fs::symlink(&source.path, &source.vendor_link).map_err(|error| {
        ConcertoError::store(
            &source.package_name,
            StoreStep::Link,
            format!("could not link vendor package to source: {error}"),
        )
    })
}

fn existing_source(
    paths: &PackagePaths,
    unsafe_trust_store: bool,
) -> Result<Option<(PathBuf, IntegrityCheck)>> {
    if !paths.published_source.exists() {
        return Ok(None);
    }

    let check = if unsafe_trust_store {
        verify_unsafe_trusted_store_marker(paths)?
    } else {
        verify_stored_integrity(paths)?
    };

    Ok(Some((published_source_dir(paths)?, check)))
}

fn download_and_publish_source(
    archive: PackageArchive<'_>,
    base_paths: &PackageBasePaths,
) -> Result<(PathBuf, String, IntegrityCheck)> {
    std::fs::create_dir_all(&base_paths.version_store).map_err(|error| {
        ConcertoError::store(
            base_paths.package_name(),
            StoreStep::Prepare,
            format!("could not create package store directory: {error}"),
        )
    })?;

    download_to_file(archive.dist_url, &base_paths.download_zip).map_err(|error| {
        ConcertoError::store_with_hint(
            base_paths.package_name(),
            StoreStep::Download,
            format!("archive {} failed: {error}", archive.dist_url),
            "Check the dist URL or retry the install.",
        )
    })?;

    let (hashes, check) = match verify_downloaded_archive(base_paths, &archive) {
        Ok(verified) => verified,
        Err(error) => {
            let _ = remove_downloaded_archive(base_paths);
            return Err(error);
        }
    };
    let paths = package_paths(base_paths, &hashes.integrity)?;

    std::fs::create_dir_all(&paths.content_store).map_err(|error| {
        ConcertoError::store(
            paths.package_name(),
            StoreStep::Prepare,
            format!("could not create package content store directory: {error}"),
        )
    })?;

    if paths.published_source.exists() {
        if archive.unsafe_trust_store {
            verify_unsafe_trusted_store_marker(&paths)?;
        } else {
            verify_stored_integrity(&paths)?;
        }
        remove_downloaded_archive(base_paths)?;

        return Ok((published_source_dir(&paths)?, hashes.integrity, check));
    }

    publish_downloaded_archive(base_paths, &paths)?;

    clean_staged_source(&paths)?;

    safe_unzip::extract_file(&paths.staged_source, &paths.zip).map_err(|error| {
        ConcertoError::store_with_hint(
            paths.package_name(),
            StoreStep::Extract,
            format!("could not extract package zip: {error}"),
            "Remove the package from .concerto/store and retry.",
        )
    })?;

    write_integrity(&paths)?;
    std::fs::rename(&paths.staged_source, &paths.published_source).map_err(|error| {
        ConcertoError::store(
            paths.package_name(),
            StoreStep::Publish,
            format!("could not publish package source: {error}"),
        )
    })?;

    let source = published_source_dir(&paths)?;

    Ok((source, hashes.integrity, check))
}

fn clean_staged_source(paths: &PackagePaths) -> Result<()> {
    if !paths.staged_source.exists() {
        return Ok(());
    }

    std::fs::remove_dir_all(&paths.staged_source).map_err(|error| {
        ConcertoError::store(
            paths.package_name(),
            StoreStep::Prepare,
            format!("could not clean temporary package source: {error}"),
        )
    })
}

fn published_source_dir(paths: &PackagePaths) -> Result<PathBuf> {
    absolute_path(only_child_dir(
        &paths.published_source,
        paths.package_name(),
    )?)
}

fn only_child_dir(path: &Path, package_name: &str) -> Result<PathBuf> {
    let dirs = std::fs::read_dir(path)
        .map_err(|error| {
            ConcertoError::store(
                package_name,
                StoreStep::Prepare,
                format!("could not read extracted package directory: {error}"),
            )
        })?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

    match dirs.as_slice() {
        [dir] => Ok(dir.clone()),
        _ => Err(ConcertoError::store(
            package_name,
            StoreStep::Extract,
            format!(
                "expected exactly one extracted directory in {}",
                path.display()
            ),
        )),
    }
}

fn publish_downloaded_archive(base_paths: &PackageBasePaths, paths: &PackagePaths) -> Result<()> {
    if paths.zip.exists() {
        std::fs::remove_file(&paths.zip).map_err(|error| {
            ConcertoError::store(
                paths.package_name(),
                StoreStep::Publish,
                format!("could not replace package archive: {error}"),
            )
        })?;
    }

    std::fs::rename(&base_paths.download_zip, &paths.zip).map_err(|error| {
        ConcertoError::store(
            paths.package_name(),
            StoreStep::Publish,
            format!("could not publish package archive: {error}"),
        )
    })
}

fn remove_downloaded_archive(base_paths: &PackageBasePaths) -> Result<()> {
    if !base_paths.download_zip.exists() {
        return Ok(());
    }

    std::fs::remove_file(&base_paths.download_zip).map_err(|error| {
        ConcertoError::store(
            base_paths.package_name(),
            StoreStep::Prepare,
            format!("could not clean downloaded archive: {error}"),
        )
    })
}

fn absolute_path(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }

    std::env::current_dir()
        .map(|current_dir| current_dir.join(path))
        .map_err(|error| ConcertoError::store("root", StoreStep::Prepare, error.to_string()))
}
