use super::{
    SelectedOutputMode, event_message, format_duration, package_count, select_output_mode,
    summary_message,
};
use crate::cli::OutputMode;
use crate::install_event::{InstallEvent, InstallEventKind, InstallSummary};
use std::time::Duration;

#[test]
fn auto_mode_uses_tui_for_interactive_terminal() {
    let mode = select_output_mode(OutputMode::Auto, true, false, "xterm-256color".to_string());

    assert_eq!(mode, SelectedOutputMode::Tui);
}

#[test]
fn auto_mode_uses_plain_output_in_ci() {
    let mode = select_output_mode(OutputMode::Auto, true, true, "xterm-256color".to_string());

    assert_eq!(mode, SelectedOutputMode::Plain);
}

#[test]
fn auto_mode_uses_plain_output_without_terminal() {
    let mode = select_output_mode(OutputMode::Auto, false, false, "xterm-256color".to_string());

    assert_eq!(mode, SelectedOutputMode::Plain);
}

#[test]
fn auto_mode_uses_plain_output_for_dumb_terminal() {
    let mode = select_output_mode(OutputMode::Auto, true, false, "dumb".to_string());

    assert_eq!(mode, SelectedOutputMode::Plain);
}

#[test]
fn explicit_ui_overrides_ci_detection() {
    let mode = select_output_mode(OutputMode::Tui, false, true, "dumb".to_string());

    assert_eq!(mode, SelectedOutputMode::Tui);
}

#[test]
fn formats_lockfile_event() {
    let event = InstallEvent {
        kind: InstallEventKind::LockfileMatched { packages: 12 },
    };

    assert_eq!(
        event_message(&event),
        "Installing from lockfile with 12 packages"
    );
}

#[test]
fn formats_summary_from_authoritative_install_result() {
    let summary = InstallSummary {
        packages: 26,
        duration: Duration::from_millis(1_234),
    };

    assert_eq!(
        summary_message(summary),
        "Install complete: 26 packages in 1.23s"
    );
}

#[test]
fn formats_short_duration_as_milliseconds() {
    assert_eq!(format_duration(Duration::from_millis(42)), "42ms");
}

#[test]
fn formats_singular_package_count() {
    assert_eq!(package_count(1), "1 package");
}
