use std::path::{Path, PathBuf};

use crate::composer::{RequiredPackage, package_path_parts, required_packages};
use crate::http::{download_bytes, get_text};
use crate::packagist::{self, PackagistRelease};

struct PackagePaths {
    vendor_link: PathBuf,
    zip: PathBuf,
    extract: PathBuf,
}

pub fn install() -> Result<(), String> {
    let content = std::fs::read_to_string("composer.json")
        .map_err(|_| "No composer.json found".to_string())?;

    let packages = required_packages(&content)?;

    std::fs::create_dir_all(".concerto/store")
        .map_err(|error| format!("Could not create local store: {error}"))?;

    std::fs::create_dir_all("vendor")
        .map_err(|error| format!("Could not create vendor directory: {error}"))?;

    for package in packages {
        install_package(&package)?;
    }

    Ok(())
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

fn install_package(package: &RequiredPackage) -> Result<(), String> {
    let paths = package_paths(package)?;
    let metadata_url = packagist::package_url(&package.name)?;
    let metadata = get_text(&metadata_url)?;
    println!("Fetched {} bytes", metadata.len());

    let release = packagist::first_release_candidate(&metadata, &package.name)?;
    print_release(&release);

    let source = download_and_extract(&release, &paths)?;
    link_vendor_package(&paths.vendor_link, &source)?;
    print_install_summary(package, &source, &metadata_url);

    Ok(())
}

fn package_paths(package: &RequiredPackage) -> Result<PackagePaths, String> {
    let (vendor, name) = package_path_parts(&package.name)?;
    let store = PathBuf::from(".concerto/store").join(vendor).join(name);
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

fn download_and_extract(
    release: &PackagistRelease,
    paths: &PackagePaths,
) -> Result<PathBuf, String> {
    let zip = download_bytes(&release.dist_url)?;

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

fn link_vendor_package(vendor_link: &Path, source: &Path) -> Result<(), String> {
    if let Ok(metadata) = std::fs::symlink_metadata(vendor_link) {
        if !metadata.file_type().is_symlink() {
            return Err(format!(
                "Vendor path already exists and is not a symlink: {}",
                vendor_link.display()
            ));
        }

        std::fs::remove_file(vendor_link)
            .map_err(|error| format!("Could not remove existing vendor link: {error}"))?;
    }

    std::os::unix::fs::symlink(source, vendor_link)
        .map_err(|error| format!("Could not link vendor package to source: {error}"))
}

fn print_release(release: &PackagistRelease) {
    println!("Found {} versions", release.version_count);
    println!("Selected {}", release.version);
    println!("Dist {}", release.dist_url);
}

fn print_install_summary(package: &RequiredPackage, source: &Path, metadata_url: &str) {
    println!(
        "{} {} -> {} ({})",
        package.name,
        package.constraint,
        source.display(),
        metadata_url
    );
}
