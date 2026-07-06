#![deny(
    clippy::cognitive_complexity,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]
#![cfg_attr(not(test), deny(clippy::expect_used, clippy::unwrap_used))]

mod autoload;
mod cli;
mod composer;
mod error;
mod http;
mod install_event;
mod installer;
mod lockfile;
mod output;
mod package_store;
mod packagist;
mod perf;
mod platform;
mod resolver;

fn main() {
    let command = cli::parse_command(std::env::args().skip(1));

    match command {
        cli::Command::Install(options) => {
            if let Err(error) = output::install(options.output_mode) {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        cli::Command::Help => print_help(),
        cli::Command::Version => print_version(),
        cli::Command::Unknown(command) => {
            eprintln!("Unknown command: {command}");
            eprintln!("{}", cli::HELP);
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!("{}", cli::HELP);
}

fn print_version() {
    println!("{}", cli::VERSION);
}
