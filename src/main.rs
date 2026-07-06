#![deny(
    clippy::cognitive_complexity,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

use std::env;

mod autoload;
mod composer;
mod error;
mod http;
mod installer;
mod lockfile;
mod package_store;
mod packagist;
mod perf;
mod platform;
mod resolver;

pub(crate) const HELP: &str = "\
Usage: concerto <COMMAND>

Commands:
  install    Install packages from composer.json

Options:
  -h, --help     Print help
  -V, --version  Print version";

pub(crate) const VERSION: &str = concat!("concerto ", env!("CARGO_PKG_VERSION"));

enum Command {
    Install,
    Help,
    Version,
    Unknown(String),
}

fn main() {
    let command = parse_command(env::args().nth(1));

    match command {
        Command::Install => {
            if let Err(error) = installer::install() {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        Command::Help => print_help(),
        Command::Version => print_version(),
        Command::Unknown(command) => {
            eprintln!("Unknown command: {command}");
            eprintln!("{HELP}");
            std::process::exit(1);
        }
    }
}

fn parse_command(argument: Option<String>) -> Command {
    match argument.as_deref() {
        Some("install") => Command::Install,
        Some("-h" | "--help") | None => Command::Help,
        Some("-V" | "--version") => Command::Version,
        Some(command) => Command::Unknown(command.to_string()),
    }
}

fn print_help() {
    println!("{HELP}");
}

fn print_version() {
    println!("{VERSION}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_install_command() {
        let command = parse_command(Some("install".to_string()));

        assert!(matches!(command, Command::Install));
    }

    #[test]
    fn falls_back_to_help_for_unknown_command() {
        let command = parse_command(Some("nope".to_string()));

        assert!(matches!(command, Command::Unknown(command) if command == "nope"));
    }

    #[test]
    fn falls_back_to_help_without_command() {
        let command = parse_command(None);

        assert!(matches!(command, Command::Help));
    }

    #[test]
    fn parses_help_flag() {
        let command = parse_command(Some("--help".to_string()));

        assert!(matches!(command, Command::Help));
    }

    #[test]
    fn parses_short_help_flag() {
        let command = parse_command(Some("-h".to_string()));

        assert!(matches!(command, Command::Help));
    }

    #[test]
    fn parses_version_flag() {
        let command = parse_command(Some("--version".to_string()));

        assert!(matches!(command, Command::Version));
    }

    #[test]
    fn parses_short_version_flag() {
        let command = parse_command(Some("-V".to_string()));

        assert!(matches!(command, Command::Version));
    }
}
