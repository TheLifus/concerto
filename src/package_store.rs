use crate::composer::package_path_parts;
use crate::http::download_bytes;
use std::path::{Path, PathBuf};

struct PackagePaths {
    vendor_link: PathBuf,
    zip: PathBuf,
    extract: PathBuf,
}

pub(crate) struct PackageSource {
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
) -> Result<PackageSource, String> {
    let paths = package_paths(package_name, archive.version)?;

    let (path, reused) = match existing_source(&paths)? {
        Some(source) => (source, true),
        None => (download_and_extract(archive, &paths)?, false),
    };

    Ok(PackageSource {
        path,
        vendor_link: paths.vendor_link,
        reused,
    })
}

pub(crate) fn link_to_vendor(source: &PackageSource) -> Result<(), String> {
    if let Ok(metadata) = std::fs::symlink_metadata(&source.vendor_link) {
        if !metadata.file_type().is_symlink() {
            return Err(format!(
                "Vendor path already exists and is not a symlink: {}",
                source.vendor_link.display()
            ));
        }

        std::fs::remove_file(&source.vendor_link)
            .map_err(|error| format!("Could not remove existing vendor link: {error}"))?;
    }

    std::os::unix::fs::symlink(&source.path, &source.vendor_link)
        .map_err(|error| format!("Could not link vendor package to source: {error}"))
}

fn package_paths(package_name: &str, version: &str) -> Result<PackagePaths, String> {
    let (vendor, name) = package_path_parts(package_name)?;
    let store = PathBuf::from(".concerto/store")
        .join(vendor)
        .join(name)
        .join(version);
    let vendor_parent = PathBuf::from("vendor").join(vendor);
    let vendor_link = vendor_parent.join(name);

    std::fs::create_dir_all(&store)
        .map_err(|error| format!("Could not create package store directory: {error}"))?;
    std::fs::create_dir_all(&vendor_parent)
        .map_err(|error| format!("Could not create vendor directory: {error}"))?;

    let store = std::fs::canonicalize(&store)
        .map_err(|error| format!("Could not resolve package store directory: {error}"))?;

    Ok(PackagePaths {
        zip: store.join("package.zip"),
        extract: store.join("source"),
        vendor_link,
    })
}

fn existing_source(paths: &PackagePaths) -> Result<Option<PathBuf>, String> {
    if !paths.extract.exists() {
        return Ok(None);
    }

    only_child_dir(&paths.extract).map(Some)
}

fn download_and_extract(
    archive: PackageArchive<'_>,
    paths: &PackagePaths,
) -> Result<PathBuf, String> {
    let zip = download_bytes(archive.dist_url)?;

    std::fs::write(&paths.zip, zip)
        .map_err(|error| format!("Could not write package zip: {error}"))?;
    println!("Downloaded {}", paths.zip.display());

    if paths.extract.exists() {
        std::fs::remove_dir_all(&paths.extract)
            .map_err(|error| format!("Could not clean package source directory: {error}"))?;
    }

    safe_unzip::extract_file(&paths.extract, &paths.zip)
        .map_err(|error| format!("Could not extract package zip: {error}"))?;
    println!("Extracted {}", paths.extract.display());

    let source = only_child_dir(&paths.extract)?;
    println!("Source {}", source.display());

    Ok(source)
}

fn only_child_dir(path: &Path) -> Result<PathBuf, String> {
    let dirs = std::fs::read_dir(path)
        .map_err(|error| format!("Could not read extracted package directory: {error}"))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

    match dirs.as_slice() {
        [dir] => Ok(dir.clone()),
        _ => Err(format!(
            "Expected exactly one extracted directory in {}",
            path.display()
        )),
    }
}
