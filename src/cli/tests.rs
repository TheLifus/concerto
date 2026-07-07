use super::{Command, OutputMode, parse_command};

fn parse(arguments: &[&str]) -> Command {
    parse_command(arguments.iter().map(|argument| argument.to_string()))
}

#[test]
fn parses_install_command() {
    let command = parse(&["install"]);

    assert!(
        matches!(command, Command::Install(options) if options.output_mode == OutputMode::Auto && options.include_dev)
    );
}

#[test]
fn parses_install_with_tui_output() {
    let command = parse(&["install", "--ui"]);

    assert!(matches!(command, Command::Install(options) if options.output_mode == OutputMode::Tui));
}

#[test]
fn parses_install_with_plain_output() {
    let command = parse(&["install", "--plain"]);

    assert!(
        matches!(command, Command::Install(options) if options.output_mode == OutputMode::Plain)
    );
}

#[test]
fn parses_install_without_progress_as_plain_output() {
    let command = parse(&["install", "--no-progress"]);

    assert!(
        matches!(command, Command::Install(options) if options.output_mode == OutputMode::Plain)
    );
}

#[test]
fn parses_install_without_dev_packages() {
    let command = parse(&["install", "--no-dev"]);

    assert!(
        matches!(command, Command::Install(options) if options.output_mode == OutputMode::Auto && !options.include_dev)
    );
}

#[test]
fn parses_help_flag() {
    let command = parse(&["--help"]);

    assert!(matches!(command, Command::Help));
}

#[test]
fn parses_short_help_flag() {
    let command = parse(&["-h"]);

    assert!(matches!(command, Command::Help));
}

#[test]
fn parses_version_flag() {
    let command = parse(&["--version"]);

    assert!(matches!(command, Command::Version));
}

#[test]
fn parses_short_version_flag() {
    let command = parse(&["-V"]);

    assert!(matches!(command, Command::Version));
}

#[test]
fn returns_unknown_command() {
    let command = parse(&["nope"]);

    assert!(matches!(command, Command::Unknown(command) if command == "nope"));
}

#[test]
fn returns_unknown_install_option() {
    let command = parse(&["install", "--wat"]);

    assert!(matches!(command, Command::Unknown(command) if command == "--wat"));
}
