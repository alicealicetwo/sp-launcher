//! Locating and starting the game.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{LauncherError, Result};

/// Relative to the install directory.
pub const GAME_EXE: &str = "BravoHotelClient.exe";

/// Folders that should sit alongside `GAME_EXE` in the install directory.
/// There is no manifest to verify file-by-file against yet, so this — plus
/// the executable itself — is the whole "is this actually installed" check:
/// good enough to recognize a folder someone already has the game in
/// (copied from Steam, a previous install, wherever) without downloading
/// anything.
const REQUIRED_DIRS: &[&str] = &["BravoHotelGame", "Engine"];

#[derive(Debug, Clone, Serialize)]
pub struct InstallState {
    pub installed: bool,
    pub exe_path: Option<String>,
}

pub fn exe_path(install_dir: &Path) -> PathBuf {
    install_dir.join(GAME_EXE)
}

pub fn detect(install_dir: &str) -> InstallState {
    if install_dir.is_empty() {
        return InstallState { installed: false, exe_path: None };
    }
    let root = Path::new(install_dir);
    let exe = exe_path(root);
    let complete = exe.is_file() && REQUIRED_DIRS.iter().all(|dir| root.join(dir).is_dir());
    if complete {
        InstallState { installed: true, exe_path: Some(exe.to_string_lossy().into_owned()) }
    } else {
        InstallState { installed: false, exe_path: None }
    }
}

/// Splits the user's arguments box into argv entries.
///
/// Newlines and spaces both separate, and double quotes group — so a path with
/// a space can be passed as `-Foo="C:\Program Files\x"`. Passing argv directly
/// (never a command string) is what keeps this free of shell-injection.
pub fn parse_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;

    for ch in raw.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(ch);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Builds the full argv passed to the game: the server address first (if the
/// person entered one in the connect prompt), then whatever they configured
/// in Launch Arguments. Split out from `launch` so it can be tested without
/// actually spawning a process.
fn full_args(server: Option<&str>, user_args: &str) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(server) = server {
        if !server.is_empty() {
            args.push(server.to_string());
        }
    }
    args.extend(parse_args(user_args));
    args
}

pub struct LaunchSpec<'a> {
    pub install_dir: &'a str,
    /// `ip:port` typed into the connect prompt, or `None`/empty for "Play
    /// without joining server" — asked fresh on every launch rather than
    /// stored, since who to connect to can change launch to launch.
    pub server: Option<&'a str>,
    pub user_args: &'a str,
}

pub fn launch(spec: LaunchSpec<'_>) -> Result<std::process::Child> {
    if spec.install_dir.is_empty() {
        return Err(LauncherError::Message("no install directory set".into()));
    }
    let exe = exe_path(Path::new(spec.install_dir));
    if !exe.is_file() {
        return Err(LauncherError::Message(format!(
            "game executable not found at {}",
            exe.display()
        )));
    }

    // The backend is reached through hosts redirection, not through arguments,
    // so the rest of the command line is just the (optional) server address
    // plus whatever the user configured.
    let args: Vec<String> = full_args(spec.server, spec.user_args);

    let working_dir = exe.parent().ok_or_else(|| {
        LauncherError::Message("game executable has no parent directory".into())
    })?;

    // Returning the Child, not just the pid: the caller waits on it so the
    // hosts entries come out again the moment the game exits.
    Command::new(&exe)
        .args(&args)
        .current_dir(working_dir)
        .spawn()
        .map_err(|e| LauncherError::Message(format!("could not start the game: {e}")))
}

#[cfg(test)]
mod tests {
    use super::{detect, full_args, parse_args};
    use std::fs;

    #[test]
    fn server_address_comes_first_when_given() {
        let got = full_args(Some("203.0.113.10:27015"), "-IgnoreCatalogue");
        assert_eq!(got, vec!["203.0.113.10:27015", "-IgnoreCatalogue"]);
    }

    #[test]
    fn no_server_address_means_just_the_user_args() {
        assert_eq!(full_args(None, "-IgnoreCatalogue"), vec!["-IgnoreCatalogue"]);
    }

    #[test]
    fn an_empty_server_address_is_treated_the_same_as_none() {
        assert_eq!(full_args(Some(""), "-IgnoreCatalogue"), vec!["-IgnoreCatalogue"]);
    }

    #[test]
    fn server_address_alone_with_no_user_args() {
        assert_eq!(full_args(Some("1.2.3.4:80"), ""), vec!["1.2.3.4:80"]);
    }

    #[test]
    fn empty_install_dir_is_never_installed() {
        assert!(!detect("").installed);
    }

    #[test]
    fn missing_folder_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        // Doesn't exist at all.
        let missing = dir.path().join("nope");
        assert!(!detect(missing.to_str().unwrap()).installed);
    }

    #[test]
    fn needs_all_three_to_count_as_installed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Nothing there yet.
        assert!(!detect(root.to_str().unwrap()).installed);

        // Just the exe: still not installed.
        fs::write(root.join("BravoHotelClient.exe"), b"").unwrap();
        assert!(!detect(root.to_str().unwrap()).installed);

        // Exe plus one of the two folders: still not installed.
        fs::create_dir(root.join("BravoHotelGame")).unwrap();
        assert!(!detect(root.to_str().unwrap()).installed);

        // All three present: installed.
        fs::create_dir(root.join("Engine")).unwrap();
        let state = detect(root.to_str().unwrap());
        assert!(state.installed);
        assert!(state.exe_path.unwrap().ends_with("BravoHotelClient.exe"));
    }

    #[test]
    fn splits_on_spaces_and_newlines() {
        let got = parse_args("-windowed -ResX=1920\n-nosteam");
        assert_eq!(got, vec!["-windowed", "-ResX=1920", "-nosteam"]);
    }

    #[test]
    fn keeps_the_default_arguments_intact() {
        // UE strips the quotes itself; the token must reach it whole.
        let got = parse_args("-IgnoreCatalogue -ApiPhase=\"dev2s\"");
        assert_eq!(got, vec!["-IgnoreCatalogue", "-ApiPhase=\"dev2s\""]);
    }

    #[test]
    fn keeps_quoted_spaces_together() {
        let got = parse_args(r#"-Path="C:\Program Files\x" -y"#);
        assert_eq!(got, vec![r#"-Path="C:\Program Files\x""#, "-y"]);
    }

    #[test]
    fn ignores_blank_runs() {
        assert_eq!(parse_args("  \n\n  -a   \n  -b  "), vec!["-a", "-b"]);
    }
}
