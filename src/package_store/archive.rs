use super::PackageArchive;
use super::paths::{PackageBasePaths, PackagePaths};
use crate::error::{ConcertoError, Result, StoreStep};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) struct ArchiveHashes {
    pub(super) integrity: String,
    shasum: Option<String>,
}

#[derive(Clone, Copy)]
enum ArchiveHashMode {
    Blake3,
    Blake3AndSha1,
}

#[derive(Clone, Copy)]
pub(crate) enum IntegrityCheckKind {
    DownloadHash,
    ReuseHash,
    UnsafeTrustedReuse,
}

#[derive(Clone, Copy)]
pub(crate) struct IntegrityCheck {
    pub kind: IntegrityCheckKind,
    pub duration: Duration,
    pub sha1: bool,
}

pub(super) fn verify_downloaded_archive(
    base_paths: &PackageBasePaths,
    archive: &PackageArchive<'_>,
) -> Result<(ArchiveHashes, IntegrityCheck)> {
    let mode = archive_hash_mode(archive.expected_shasum);
    let started_at = Instant::now();
    let hashes = archive_hashes(base_paths.package_name(), &base_paths.download_zip, mode)?;
    verify_integrity(
        base_paths.package_name(),
        archive.expected_integrity,
        &hashes.integrity,
    )?;
    verify_shasum(
        base_paths.package_name(),
        archive.expected_shasum,
        hashes.shasum.as_deref(),
    )?;

    Ok((
        hashes,
        IntegrityCheck {
            kind: IntegrityCheckKind::DownloadHash,
            duration: started_at.elapsed(),
            sha1: matches!(mode, ArchiveHashMode::Blake3AndSha1),
        },
    ))
}

pub(super) fn verify_unsafe_trusted_store_marker(paths: &PackagePaths) -> Result<IntegrityCheck> {
    let started_at = Instant::now();
    verify_integrity_marker(paths)?;

    Ok(IntegrityCheck {
        kind: IntegrityCheckKind::UnsafeTrustedReuse,
        duration: started_at.elapsed(),
        sha1: false,
    })
}

pub(super) fn verify_stored_integrity(paths: &PackagePaths) -> Result<IntegrityCheck> {
    let started_at = Instant::now();
    verify_integrity_marker(paths)?;
    let hashes = archive_hashes(paths.package_name(), &paths.zip, ArchiveHashMode::Blake3)?;

    verify_integrity(
        paths.package_name(),
        Some(&paths.content_integrity),
        &hashes.integrity,
    )?;

    Ok(IntegrityCheck {
        kind: IntegrityCheckKind::ReuseHash,
        duration: started_at.elapsed(),
        sha1: false,
    })
}

pub(super) fn write_integrity(paths: &PackagePaths) -> Result<()> {
    std::fs::write(&paths.integrity, format!("{}\n", paths.content_integrity)).map_err(|error| {
        ConcertoError::store(
            paths.package_name(),
            StoreStep::Publish,
            format!("could not write archive integrity marker: {error}"),
        )
    })
}

fn verify_integrity_marker(paths: &PackagePaths) -> Result<()> {
    let integrity = std::fs::read_to_string(&paths.integrity)
        .map(|content| content.trim().to_string())
        .map_err(|error| {
            ConcertoError::store_with_hint(
                paths.package_name(),
                StoreStep::Prepare,
                format!("missing archive integrity marker: {error}"),
                "Remove the package from .concerto/store and run install again.",
            )
        })?;

    verify_integrity(
        paths.package_name(),
        Some(&paths.content_integrity),
        &integrity,
    )
}

fn verify_integrity(
    package_name: &str,
    expected_integrity: Option<&str>,
    observed_integrity: &str,
) -> Result<()> {
    let Some(expected_integrity) = expected_integrity else {
        return Ok(());
    };

    if observed_integrity == expected_integrity {
        return Ok(());
    }

    Err(ConcertoError::store_with_hint(
        package_name,
        StoreStep::Download,
        format!(
            "archive integrity mismatch: expected {expected_integrity}, observed {observed_integrity}"
        ),
        "Remove the package from .concerto/store and retry. If the archive changed upstream, regenerate concerto.lock intentionally.",
    ))
}

fn verify_shasum(
    package_name: &str,
    expected_shasum: Option<&str>,
    observed_shasum: Option<&str>,
) -> Result<()> {
    let Some(expected_shasum) = expected_shasum else {
        return Ok(());
    };
    let Some(observed_shasum) = observed_shasum else {
        return Err(ConcertoError::store(
            package_name,
            StoreStep::Download,
            "archive shasum could not be computed",
        ));
    };

    if observed_shasum.eq_ignore_ascii_case(expected_shasum) {
        return Ok(());
    }

    Err(ConcertoError::store_with_hint(
        package_name,
        StoreStep::Download,
        format!("archive shasum mismatch: expected {expected_shasum}, observed {observed_shasum}"),
        "The package archive does not match Packagist metadata. Retry later or choose another version.",
    ))
}

fn archive_hashes(package_name: &str, path: &Path, mode: ArchiveHashMode) -> Result<ArchiveHashes> {
    let mut file = std::fs::File::open(path).map_err(|error| {
        ConcertoError::store(
            package_name,
            StoreStep::Download,
            format!("could not read downloaded archive: {error}"),
        )
    })?;
    let mut blake3 = blake3::Hasher::new();
    let mut sha1 = matches!(mode, ArchiveHashMode::Blake3AndSha1).then(sha1_smol::Sha1::new);
    let mut buffer = [0; 64 * 1024];

    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            ConcertoError::store(
                package_name,
                StoreStep::Download,
                format!("could not hash downloaded archive: {error}"),
            )
        })?;

        if read == 0 {
            break;
        }

        blake3.update(&buffer[..read]);
        if let Some(sha1) = &mut sha1 {
            sha1.update(&buffer[..read]);
        }
    }

    Ok(ArchiveHashes {
        integrity: format!("blake3:{}", blake3.finalize().to_hex()),
        shasum: sha1.map(|sha1| sha1.digest().to_string()),
    })
}

fn archive_hash_mode(expected_shasum: Option<&str>) -> ArchiveHashMode {
    if expected_shasum.is_some() {
        ArchiveHashMode::Blake3AndSha1
    } else {
        ArchiveHashMode::Blake3
    }
}
