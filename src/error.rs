use std::fmt;

pub(crate) type Result<T> = std::result::Result<T, ConcertoError>;

struct ErrorReport {
    summary: String,
    hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConcertoError {
    MissingComposerJson,
    ComposerJson {
        message: String,
    },
    Requirement {
        message: String,
    },
    InvalidPackageName {
        name: String,
    },
    Http {
        message: String,
    },
    Lockfile {
        message: String,
    },
    Autoload {
        message: String,
    },
    Perf {
        message: String,
    },
    Platform {
        package: String,
        requirement: String,
        detected: String,
    },
    PlatformDetection {
        message: String,
    },
    Resolution {
        package: String,
        constraints: Vec<String>,
        message: String,
    },
    Store {
        package: String,
        step: StoreStep,
        message: String,
        hint: Option<String>,
    },
    Internal {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoreStep {
    Prepare,
    Download,
    Extract,
    Publish,
    Link,
    Rollback,
}

impl ConcertoError {
    pub(crate) fn composer_json(message: impl Into<String>) -> Self {
        Self::ComposerJson {
            message: message.into(),
        }
    }

    pub(crate) fn requirement(message: impl Into<String>) -> Self {
        Self::Requirement {
            message: message.into(),
        }
    }

    pub(crate) fn invalid_package_name(name: impl Into<String>) -> Self {
        Self::InvalidPackageName { name: name.into() }
    }

    pub(crate) fn http(message: impl Into<String>) -> Self {
        Self::Http {
            message: message.into(),
        }
    }

    pub(crate) fn lockfile(message: impl Into<String>) -> Self {
        Self::Lockfile {
            message: message.into(),
        }
    }

    pub(crate) fn autoload(message: impl Into<String>) -> Self {
        Self::Autoload {
            message: message.into(),
        }
    }

    pub(crate) fn perf(message: impl Into<String>) -> Self {
        Self::Perf {
            message: message.into(),
        }
    }

    pub(crate) fn platform_detection(message: impl Into<String>) -> Self {
        Self::PlatformDetection {
            message: message.into(),
        }
    }

    pub(crate) fn platform(
        package: impl Into<String>,
        requirement: impl Into<String>,
        detected: impl Into<String>,
    ) -> Self {
        Self::Platform {
            package: package.into(),
            requirement: requirement.into(),
            detected: detected.into(),
        }
    }

    pub(crate) fn resolution(
        package: impl Into<String>,
        constraints: &[String],
        message: impl Into<String>,
    ) -> Self {
        Self::Resolution {
            package: package.into(),
            constraints: constraints.to_vec(),
            message: message.into(),
        }
    }

    pub(crate) fn store(
        package: impl Into<String>,
        step: StoreStep,
        message: impl Into<String>,
    ) -> Self {
        Self::Store {
            package: package.into(),
            step,
            message: message.into(),
            hint: None,
        }
    }

    pub(crate) fn store_with_hint(
        package: impl Into<String>,
        step: StoreStep,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::Store {
            package: package.into(),
            step,
            message: message.into(),
            hint: Some(hint.into()),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }

    fn report(&self) -> ErrorReport {
        match self {
            Self::MissingComposerJson => ErrorReport::with_hint(
                "No composer.json found",
                "Run concerto from a Composer project directory.",
            ),
            Self::ComposerJson { message } => {
                ErrorReport::with_hint(message, "Check composer.json and its require section.")
            }
            Self::Requirement { message } => ErrorReport::summary(message),
            Self::InvalidPackageName { name } => {
                ErrorReport::summary(format!("Invalid package name: {name}"))
            }
            Self::Http { message } => ErrorReport::summary(message),
            Self::Lockfile { message } => ErrorReport::with_hint(
                message,
                "Regenerate concerto.lock by running install again.",
            ),
            Self::Autoload { message } => {
                ErrorReport::with_hint(message, "Check installed packages and autoload sections.")
            }
            Self::Perf { message } => ErrorReport::summary(message),
            Self::Platform {
                package,
                requirement,
                detected,
            } => ErrorReport::with_hint(
                format!("{package}: {requirement} required, detected {detected}"),
                "Install or enable the required platform dependency, or choose a compatible version.",
            ),
            Self::PlatformDetection { message } => {
                ErrorReport::with_hint(message, "Ensure PHP is installed and available in PATH.")
            }
            Self::Resolution {
                package,
                constraints,
                message,
            } => ErrorReport::with_hint(
                format!(
                    "Could not resolve {package} ({}): {message}",
                    constraints.join(", ")
                ),
                "Check the package name, version constraint, and platform requirements.",
            ),
            Self::Store {
                package,
                step,
                message,
                hint,
            } => ErrorReport {
                summary: format!("Could not {step} {package}: {message}"),
                hint: hint.clone(),
            },
            Self::Internal { message } => ErrorReport::summary(message),
        }
    }
}

impl ErrorReport {
    fn summary(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            hint: None,
        }
    }

    fn with_hint(summary: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            hint: Some(hint.into()),
        }
    }
}

impl fmt::Display for ConcertoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.report().fmt(formatter)
    }
}

impl std::error::Error for ConcertoError {}

impl fmt::Display for ErrorReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.summary)?;

        if let Some(hint) = &self.hint {
            write!(formatter, ". {hint}")?;
        }

        Ok(())
    }
}

impl fmt::Display for StoreStep {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let step = match self {
            Self::Prepare => "prepare",
            Self::Download => "download",
            Self::Extract => "extract",
            Self::Publish => "publish",
            Self::Link => "link",
            Self::Rollback => "rollback",
        };

        formatter.write_str(step)
    }
}
