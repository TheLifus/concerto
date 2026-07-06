use crate::composer::package_path_parts;
use crate::error::{ConcertoError, Result, StoreStep};
use crate::http::download_bytes;
use std::path::{Path, PathBuf};

struct PackagePaths {
    package_name: String,
    vendor_link: PathBuf,
    zip: PathBuf,
    published_source: PathBuf,
    staged_source: PathBuf,
}

pub(crate) struct PackageSource {
    package_name: String,
    path: PathBuf,
    vendor_link: PathBuf,
    reused: bool,
}

pub(crate) struct PackageArchive<'a> {
    pub version: &'a str,
    pub dist_url: &'a str,
}

impl PackageSource {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn is_reused(&self) -> bool {
        self.reused
    }
}

pub(crate) fn prepare_source(
    package_name: &str,
    archive: PackageArchive<'_>,
) -> Result<PackageSource> {
    let paths = package_paths(package_name, archive.version)?;

    let (path, reused) = match existing_source(&paths)? {
        Some(source) => (source, true),
        None => (download_and_publish_source(archive, &paths)?, false),
    };

    Ok(PackageSource {
        package_name: package_name.to_string(),
        path,
        vendor_link: paths.vendor_link,
        reused,
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

    std::os::unix::fs::symlink(&source.path, &source.vendor_link).map_err(|error| {
        ConcertoError::store(
            &source.package_name,
            StoreStep::Link,
            format!("could not link vendor package to source: {error}"),
        )
    })
}

fn package_paths(package_name: &str, version: &str) -> Result<PackagePaths> {
    let (vendor, name) = package_path_parts(package_name).map_err(|error| {
        ConcertoError::store(package_name, StoreStep::Prepare, error.to_string())
    })?;
    let store = PathBuf::from(".concerto/store")
        .join(vendor)
        .join(name)
        .join(version);
    let vendor_parent = PathBuf::from("vendor").join(vendor);
    let vendor_link = vendor_parent.join(name);

    std::fs::create_dir_all(&store).map_err(|error| {
        ConcertoError::store(
            package_name,
            StoreStep::Prepare,
            format!("could not create package store directory: {error}"),
        )
    })?;
    std::fs::create_dir_all(&vendor_parent).map_err(|error| {
        ConcertoError::store(
            package_name,
            StoreStep::Prepare,
            format!("could not create vendor directory: {error}"),
        )
    })?;

    let store = std::fs::canonicalize(&store).map_err(|error| {
        ConcertoError::store(
            package_name,
            StoreStep::Prepare,
            format!("could not resolve package store directory: {error}"),
        )
    })?;

    Ok(PackagePaths {
        package_name: package_name.to_string(),
        zip: store.join("package.zip"),
        published_source: store.join("source"),
        staged_source: store.join("source.tmp"),
        vendor_link,
    })
}

impl PackagePaths {
    fn package_name(&self) -> &str {
        &self.package_name
    }
}

fn existing_source(paths: &PackagePaths) -> Result<Option<PathBuf>> {
    if !paths.published_source.exists() {
        return Ok(None);
    }

    published_source_dir(paths).map(Some)
}

fn download_and_publish_source(
    archive: PackageArchive<'_>,
    paths: &PackagePaths,
) -> Result<PathBuf> {
    let zip = download_bytes(archive.dist_url).map_err(|error| {
        ConcertoError::store_with_hint(
            paths.package_name(),
            StoreStep::Download,
            format!("archive {} failed: {error}", archive.dist_url),
            "Check the dist URL or retry the install.",
        )
    })?;

    std::fs::write(&paths.zip, zip).map_err(|error| {
        ConcertoError::store(
            paths.package_name(),
            StoreStep::Download,
            format!("could not write package zip: {error}"),
        )
    })?;

    clean_staged_source(paths)?;

    safe_unzip::extract_file(&paths.staged_source, &paths.zip).map_err(|error| {
        ConcertoError::store_with_hint(
            paths.package_name(),
            StoreStep::Extract,
            format!("could not extract package zip: {error}"),
            "Remove the package from .concerto/store and retry.",
        )
    })?;

    // Published sources may be shared by vendor links, so never delete them.
    if paths.published_source.exists() {
        clean_staged_source(paths)?;

        return published_source_dir(paths);
    }

    std::fs::rename(&paths.staged_source, &paths.published_source).map_err(|error| {
        ConcertoError::store(
            paths.package_name(),
            StoreStep::Publish,
            format!("could not publish package source: {error}"),
        )
    })?;

    let source = published_source_dir(paths)?;

    Ok(source)
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
    only_child_dir(&paths.published_source, paths.package_name())
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
