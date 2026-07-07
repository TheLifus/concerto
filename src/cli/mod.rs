#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputMode {
    Auto,
    Plain,
    Tui,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct InstallOptions {
    pub output_mode: OutputMode,
    pub include_dev: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Command {
    Install(InstallOptions),
    Help,
    Version,
    Unknown(String),
}

pub(crate) const HELP: &str = "\
Usage: concerto <COMMAND>

Commands:
  install    Install packages from composer.json

Install options:
      --ui              Force terminal UI output
      --plain           Force plain text output
      --no-progress     Disable progress UI
      --no-dev          Skip composer.json require-dev packages

Global options:
  -h, --help     Print help
  -V, --version  Print version";

pub(crate) const VERSION: &str = concat!("concerto ", env!("CARGO_PKG_VERSION"));

pub(crate) fn parse_command(arguments: impl IntoIterator<Item = String>) -> Command {
    let mut arguments = arguments.into_iter();

    match arguments.next().as_deref() {
        Some("install") => parse_install_options(arguments),
        Some("-h" | "--help") | None => Command::Help,
        Some("-V" | "--version") => Command::Version,
        Some(command) => Command::Unknown(command.to_string()),
    }
}

fn parse_install_options(arguments: impl IntoIterator<Item = String>) -> Command {
    let mut output_mode = OutputMode::Auto;
    let mut include_dev = true;

    for argument in arguments {
        match argument.as_str() {
            "--ui" => output_mode = OutputMode::Tui,
            "--plain" | "--no-progress" => output_mode = OutputMode::Plain,
            "--no-dev" => include_dev = false,
            "-h" | "--help" => return Command::Help,
            unknown => return Command::Unknown(unknown.to_string()),
        }
    }

    Command::Install(InstallOptions {
        output_mode,
        include_dev,
    })
}

#[cfg(test)]
mod tests;
