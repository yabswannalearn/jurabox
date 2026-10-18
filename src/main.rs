#[cfg(windows)]
mod command_keeper;
mod platform;
mod projects;
#[cfg(windows)]
mod recovery;
#[cfg(windows)]
mod services;
#[cfg(windows)]
mod session_host;
mod ui;

use anyhow::{Result, bail};
use std::process::ExitCode;

#[derive(Debug, PartialEq)]
enum Command {
    App {
        hidden: bool,
    },
    Status,
    DesktopCheck,
    UiCheck,
    WorkspaceCheck,
    AssignmentCheck,
    ServiceCheck,
    ServiceUiCheck,
    ComposeCheck,
    SessionCheck,
    RecoveryCheck,
    Keeper,
    SessionHost {
        journal: std::path::PathBuf,
    },
    ServiceFixture {
        mode: String,
        folder: std::path::PathBuf,
    },
    ProbeWindow,
    Help,
}

fn parse_command(args: &[String]) -> Result<Command> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["app"] => Ok(Command::App { hidden: false }),
        ["--tray"] => Ok(Command::App { hidden: true }),
        ["status"] => Ok(Command::Status),
        ["ui-check"] => Ok(Command::UiCheck),
        ["workspace-check", "--exercise"] => Ok(Command::WorkspaceCheck),
        ["assignment-check", "--exercise"] => Ok(Command::AssignmentCheck),
        ["service-check", "--exercise"] => Ok(Command::ServiceCheck),
        ["service-ui-check", "--exercise"] => Ok(Command::ServiceUiCheck),
        ["compose-check", "--exercise"] => Ok(Command::ComposeCheck),
        ["session-check", "--exercise"] => Ok(Command::SessionCheck),
        ["recovery-check", "--exercise"] => Ok(Command::RecoveryCheck),
        ["_command-keeper"] => Ok(Command::Keeper),
        ["_session-host", journal] => Ok(Command::SessionHost {
            journal: (*journal).into(),
        }),
        ["_service-fixture", mode, folder] => Ok(Command::ServiceFixture {
            mode: (*mode).into(),
            folder: (*folder).into(),
        }),
        ["_probe-window"] => Ok(Command::ProbeWindow),
        ["desktop-check", "--exercise"] => Ok(Command::DesktopCheck),
        ["--help"] | ["-h"] | ["help"] => Ok(Command::Help),
        ["desktop-check"]
        | ["workspace-check"]
        | ["assignment-check"]
        | ["service-check"]
        | ["service-ui-check"]
        | ["compose-check"]
        | ["session-check"]
        | ["recovery-check"] => bail!(
            "This check requires --exercise: it temporarily switches desktops and opens test windows"
        ),
        _ => bail!("unknown command; run jurabox --help for usage"),
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match parse_command(&args)? {
        Command::App { hidden } => ui::run(hidden),
        Command::Status => platform::status(),
        Command::DesktopCheck => platform::desktop_check(),
        Command::UiCheck => ui::check(),
        Command::WorkspaceCheck => platform::workspace_check(),
        Command::AssignmentCheck => ui::assignment_check(),
        Command::ServiceUiCheck => ui::service_check(),
        Command::SessionHost { journal } => {
            #[cfg(windows)]
            {
                session_host::run(&journal)
            }
            #[cfg(not(windows))]
            {
                let _ = journal;
                bail!("Session hosts require Windows 11.")
            }
        }
        Command::SessionCheck => {
            #[cfg(windows)]
            {
                session_host::check()
            }
            #[cfg(not(windows))]
            {
                bail!("Session checks require Windows 11.")
            }
        }
        Command::RecoveryCheck => {
            #[cfg(windows)]
            {
                session_host::recovery_check()
            }
            #[cfg(not(windows))]
            {
                bail!("Recovery checks require Windows 11.")
            }
        }
        Command::Keeper => {
            #[cfg(windows)]
            {
                command_keeper::run()
            }
            #[cfg(not(windows))]
            {
                bail!("Command keepers require Windows 11.")
            }
        }
        Command::ComposeCheck => {
            #[cfg(windows)]
            {
                services::compose_check()
            }
            #[cfg(not(windows))]
            {
                bail!("Compose checks require Windows 11.")
            }
        }
        Command::ServiceCheck => {
            #[cfg(windows)]
            {
                services::check()
            }
            #[cfg(not(windows))]
            {
                bail!("Service checks require Windows 11.")
            }
        }
        Command::ServiceFixture { mode, folder } => {
            #[cfg(windows)]
            {
                services::fixture(&mode, &folder)
            }
            #[cfg(not(windows))]
            {
                let _ = (mode, folder);
                bail!("Service fixtures require Windows 11.")
            }
        }
        Command::ProbeWindow => platform::probe_window(),
        Command::Help => {
            println!(
                "Jurabox\n\n\
                 jurabox                            Open the project manager (default)\n\
                 jurabox --tray                     Start quietly in the tray\n\
                 jurabox status                     List desktops without changing them\n\
                 jurabox desktop-check --exercise   Verify a temporary desktop and owned test window\n\n\
                 jurabox ui-check                   Check UI using temporary settings\n\n\
                 jurabox workspace-check --exercise Verify app launching and workspace switching\n\n\
                 jurabox assignment-check --exercise Verify manual window assignment with an owned fixture\n\n\
                 jurabox service-check --exercise   Verify owned command services with finite fixtures\n\n\
                 jurabox service-ui-check --exercise Verify service controls in a temporary workspace\n\n\
                 jurabox compose-check --exercise   Verify private Docker Compose containers\n\n\
                 jurabox session-check --exercise   Verify background sessions and reconnection\n\n\
                 jurabox recovery-check --exercise  Verify recovery after a private host crash\n\n\
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
    fn default_command_opens_manager_without_launching_projects() {
        assert_eq!(parse_command(&[]).unwrap(), Command::App { hidden: false });
    }

    #[test]
    fn desktop_changes_require_explicit_exercise_flag() {
        assert!(parse_command(&args(&["desktop-check"])).is_err());
        assert!(parse_command(&args(&["workspace-check"])).is_err());
        assert!(parse_command(&args(&["assignment-check"])).is_err());
        assert!(parse_command(&args(&["service-check"])).is_err());
        assert!(parse_command(&args(&["service-ui-check"])).is_err());
        assert_eq!(
            parse_command(&args(&["workspace-check", "--exercise"])).unwrap(),
            Command::WorkspaceCheck
        );
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
