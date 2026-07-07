mod plain;
mod tui;

use crate::cli::{InstallOptions, OutputMode};
use crate::error::{ConcertoError, Result};
use crate::install_event::{InstallEvent, InstallReporter, InstallSummary};
use crate::installer;
use std::io::IsTerminal;
use std::sync::mpsc;
use std::time::Duration;

pub(crate) fn install(options: InstallOptions) -> Result<()> {
    let requested_mode = options.output_mode;
    let is_terminal = std::io::stdout().is_terminal();
    let mut selected_mode = select_output_mode(
        requested_mode,
        is_terminal,
        std::env::var_os("CI").is_some(),
        std::env::var("TERM").unwrap_or_default(),
    );

    if requested_mode == OutputMode::Tui && !is_terminal {
        return Err(ConcertoError::internal(
            "Cannot start terminal UI without an interactive terminal",
        ));
    }

    let terminal = match selected_mode {
        SelectedOutputMode::Plain => None,
        SelectedOutputMode::Tui => match tui::start() {
            Ok(terminal) => Some(terminal),
            Err(_) if requested_mode == OutputMode::Auto => {
                selected_mode = SelectedOutputMode::Plain;
                None
            }
            Err(error) => return Err(error),
        },
    };

    let (event_sender, event_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    let reporter = InstallReporter::new(event_sender);

    std::thread::spawn(move || {
        let result = installer::install(reporter, options.include_dev);
        let _ = result_sender.send(result);
    });

    match selected_mode {
        SelectedOutputMode::Plain => plain::run(event_receiver, result_receiver),
        SelectedOutputMode::Tui => {
            let terminal = terminal
                .ok_or_else(|| ConcertoError::internal("Terminal UI was not initialized"))?;

            tui::run(terminal, event_receiver, result_receiver)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SelectedOutputMode {
    Plain,
    Tui,
}

fn select_output_mode(
    requested_mode: OutputMode,
    is_terminal: bool,
    is_ci: bool,
    term: String,
) -> SelectedOutputMode {
    match requested_mode {
        OutputMode::Plain => SelectedOutputMode::Plain,
        OutputMode::Tui => SelectedOutputMode::Tui,
        OutputMode::Auto if is_terminal && !is_ci && term != "dumb" => SelectedOutputMode::Tui,
        OutputMode::Auto => SelectedOutputMode::Plain,
    }
}

fn recv_finished(
    receiver: &mpsc::Receiver<Result<InstallSummary>>,
) -> std::result::Result<Option<Result<InstallSummary>>, mpsc::TryRecvError> {
    receiver.try_recv().map(Some).or_else(|error| match error {
        mpsc::TryRecvError::Empty => Ok(None),
        mpsc::TryRecvError::Disconnected => Err(error),
    })
}

fn worker_disconnected() -> ConcertoError {
    ConcertoError::internal("Install worker stopped before reporting a result")
}

fn event_message(event: &InstallEvent) -> String {
    match &event.kind {
        crate::install_event::InstallEventKind::Started => "Starting install".to_string(),
        crate::install_event::InstallEventKind::PlatformDetected {
            php_version,
            extension_count,
        } => format!("Platform php {php_version}, {extension_count} extensions"),
        crate::install_event::InstallEventKind::LockfileMatched { packages } => {
            format!("Installing from lockfile with {}", package_count(*packages))
        }
        crate::install_event::InstallEventKind::LockfileOutdated => {
            "Ignoring outdated lockfile".to_string()
        }
        crate::install_event::InstallEventKind::MetadataFetched { package, bytes } => {
            format!("{package}: fetched {bytes} bytes")
        }
        crate::install_event::InstallEventKind::PackageResolved {
            package,
            version,
            version_count,
            package_requirements,
            platform_requirements,
            dist_url,
        } => format!(
            "{package}: selected {version} from {version_count} versions, \
             {package_requirements} package requirements, \
             {platform_requirements} platform requirements ({dist_url})"
        ),
        crate::install_event::InstallEventKind::SourceReused { package, path } => {
            format!("{package}: Reusing {path}")
        }
        crate::install_event::InstallEventKind::SourcePrepared { package, path } => {
            format!("{package}: prepared {path}")
        }
        crate::install_event::InstallEventKind::VendorLinked {
            package,
            version,
            path,
        } => format!("{package} {version} -> {path}"),
        crate::install_event::InstallEventKind::AutoloadWritten { packages } => {
            format!("Generated autoload for {packages} packages")
        }
        crate::install_event::InstallEventKind::LockfileWritten => {
            "Wrote concerto.lock".to_string()
        }
    }
}

fn summary_message(summary: InstallSummary) -> String {
    format!(
        "Install complete: {} in {}",
        package_count(summary.packages),
        format_duration(summary.duration)
    )
}

fn format_duration(duration: Duration) -> String {
    let millis = duration.as_millis();

    if millis < 1_000 {
        return format!("{millis}ms");
    }

    let seconds = millis as f64 / 1_000.0;

    format!("{seconds:.2}s")
}

fn package_count(packages: usize) -> String {
    if packages == 1 {
        return "1 package".to_string();
    }

    format!("{packages} packages")
}

#[cfg(test)]
mod tests;
