mod platform;

use anyhow::{Result, bail};
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
enum Command {
    Status,
    DesktopCheck,
    Help,
}

fn parse_command(args: &[String]) -> Result<Command> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["status"] => Ok(Command::Status),
        ["desktop-check", "--exercise"] => Ok(Command::DesktopCheck),
        ["--help"] | ["-h"] | ["help"] => Ok(Command::Help),
        ["desktop-check"] => bail!(
            "desktop-check requires --exercise: it temporarily switches desktops and opens a test window"
        ),
        _ => bail!("unknown command; run jurabox --help for usage"),
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match parse_command(&args)? {
        Command::Status => platform::status(),
        Command::DesktopCheck => platform::desktop_check(),
        Command::Help => {
            println!(
                "Jurabox compatibility tool\n\n\
                 jurabox status                     List desktops without changing them (default)\n\
                 jurabox desktop-check --exercise   Verify a temporary desktop and owned test window\n\n\
                 The exercise briefly switches desktops, restores the starting desktop,\n\
                 closes its test window, and removes only its temporary desktop."
            );
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Jurabox: {error:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn default_command_does_not_change_desktops() {
        assert_eq!(parse_command(&[]).unwrap(), Command::Status);
    }

    #[test]
    fn desktop_changes_require_explicit_exercise_flag() {
        assert!(parse_command(&args(&["desktop-check"])).is_err());
        assert_eq!(
            parse_command(&args(&["desktop-check", "--exercise"])).unwrap(),
            Command::DesktopCheck
        );
    }

    #[test]
    fn unknown_arguments_cannot_start_an_exercise() {
        assert!(parse_command(&args(&["status", "--exercise"])).is_err());
        assert!(parse_command(&args(&["desktop-check", "--force"])).is_err());
    }
}
