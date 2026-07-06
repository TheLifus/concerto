#![deny(
    clippy::cognitive_complexity,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

use std::env;

mod composer;
mod http;
mod installer;
mod lockfile;
mod package_store;
mod packagist;
mod perf;
mod platform;
mod resolver;

pub(crate) const USAGE: &str = "Usage: concerto install";

enum Command {
    Install,
    Help,
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
    }
}

fn parse_command(argument: Option<String>) -> Command {
    match argument.as_deref() {
        Some("install") => Command::Install,
        _ => Command::Help,
    }
}

fn print_help() {
    println!("{USAGE}");
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

        assert!(matches!(command, Command::Help));
    }

    #[test]
    fn falls_back_to_help_without_command() {
        let command = parse_command(None);

        assert!(matches!(command, Command::Help));
    }
}
