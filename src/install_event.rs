use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct InstallReporter {
    sender: mpsc::Sender<InstallEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InstallEvent {
    pub kind: InstallEventKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InstallSummary {
    pub packages: usize,
    pub duration: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InstallEventKind {
    Started,
    PlatformDetected {
        php_version: String,
        extension_count: usize,
    },
    LockfileMatched {
        packages: usize,
    },
    LockfileOutdated,
    MetadataFetched {
        package: String,
        bytes: usize,
    },
    PackageResolved {
        package: String,
        version: String,
        version_count: usize,
        package_requirements: usize,
        platform_requirements: usize,
        dist_url: String,
    },
    SourceReused {
        package: String,
        path: String,
    },
    SourcePrepared {
        package: String,
        path: String,
    },
    VendorLinked {
        package: String,
        version: String,
        path: String,
    },
    AutoloadWritten {
        packages: usize,
    },
    LockfileWritten,
}

impl InstallReporter {
    pub(crate) fn new(sender: mpsc::Sender<InstallEvent>) -> Self {
        Self { sender }
    }

    pub(crate) fn emit(&self, kind: InstallEventKind) {
        let _ = self.sender.send(InstallEvent { kind });
    }

    pub(crate) fn path(path: &Path) -> String {
        path.display().to_string()
    }
}
