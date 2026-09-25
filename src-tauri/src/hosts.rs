//! Windows hosts-file redirection.
//!
//! The game resolves real bravohotel.io hostnames, so pointing it at a private
//! backend means overriding name resolution. The launcher owns a single marked
//! block and never touches anything outside it:
//!
//! ```text
//! # sp-launcher start
//! 127.0.0.1 game.bravohotel.io
//! # sp-launcher end
//! ```
//!
//! Since 0.3.0 the block is PERMANENT: it is written once and left in place.
//! The names only belong to the shut-down official service, so keeping them
//! redirected costs nothing, and it lets the launcher run as a normal
//! (non-admin) app. Writing the file needs admin rights, so when the block is
//! missing or outdated `ensure` starts a copy of the launcher elevated
//! (`--sp-hosts apply`, one UAC prompt) that only edits the hosts file and
//! exits. The original file is backed up before the first edit; "Remove
//! entries" in Settings takes the block out again the same way.
//!
//! The block editing is pure string work and unit-tested; only the file IO,
//! the elevation and the DNS flush are Windows-specific.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{LauncherError, Result};

pub const BEGIN: &str = "# sp-launcher start";
pub const END: &str = "# sp-launcher end";

/// What the hostnames below are pointed at. Fixed: this launcher exists to
/// reach one backend, and a mistyped IP here is indistinguishable, from the
/// player's side, from the server being down.
pub const BACKEND_IP: &str = "64.226.112.204";

/// The hostnames the game talks to, which the block above overrides.
const DOMAINS: &[&str] = &[
    "game.bravohotel.io",
    "ui-lobby.bravohotel.io",
    "game-public-dev2-ap-northeast-2.bravohotel.io",
    "game-private-dev.bravohotel.io",
];

/// `DOMAINS` as owned strings, which is what the functions here take.
pub fn domains() -> Vec<String> {
    DOMAINS.iter().map(|d| (*d).to_string()).collect()
}

/// Written while the block is applied, so a crashed launcher can be detected
/// and cleaned up on the next start.
const SENTINEL: &str = "hosts-applied.json";

/// Format of the pre-0.3.0 sentinel file; kept for reference only.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Applied {
    pub ip: String,
    pub hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostsStatus {
    pub path: String,
    /// Can we actually write the file? False means the launcher is not elevated.
    pub writable: bool,
    /// Is our block currently in the file?
    pub applied: bool,
    /// Lines outside our block that map one of the same hostnames. These would
    /// win or lose unpredictably, so the UI surfaces them rather than silently
    /// editing lines the user wrote by hand.
    pub conflicts: Vec<String>,
}

pub fn hosts_path() -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    Path::new(&root).join(r"System32\drivers\etc\hosts")
}

// ----------------------------------------------------------- pure editing ---

/// Keep the file's existing line ending so a CRLF hosts file stays CRLF.
fn line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

pub fn build_block(ip: &str, hosts: &[String], eol: &str) -> String {
    let mut out = String::new();
    out.push_str(BEGIN);
    out.push_str(eol);
    for host in hosts {
        out.push_str(ip);
        out.push(' ');
        out.push_str(host);
        out.push_str(eol);
    }
    out.push_str(END);
    out.push_str(eol);
    out
}

/// Remove our block. A block with a start but no end (killed mid-write) is
/// removed to end of file, since everything after the marker was ours.
pub fn strip_block(content: &str) -> String {
    let eol = line_ending(content);
    let mut out: Vec<&str> = Vec::new();
    let mut inside = false;

    for line in content.split_inclusive('\n') {
        let trimmed = line.trim();
        if !inside && trimmed == BEGIN {
            inside = true;
            continue;
        }
        if inside {
            if trimmed == END {
                inside = false;
            }
            continue;
        }
        out.push(line);
    }

    let mut joined: String = out.concat();
    // Collapse the blank run a removed block can leave behind.
    while joined.ends_with("\n\n") || joined.ends_with("\r\n\r\n") {
        joined.truncate(joined.len() - eol.len());
    }
    joined
}

pub fn has_block(content: &str) -> bool {
    content.lines().any(|l| l.trim() == BEGIN)
}

pub fn with_block(content: &str, ip: &str, hosts: &[String]) -> String {
    let eol = line_ending(content);
    let mut base = strip_block(content);
    // Our own block always goes last, so anything else in the file mapping
    // the same hostnames would otherwise win (Windows uses the first match).
    // Comment those lines out rather than deleting them, so a plain toggle
    // off restores the file exactly instead of losing hand-written entries.
    base = disable_conflicts(&base, hosts);
    if !base.is_empty() && !base.ends_with('\n') {
        base.push_str(eol);
    }
    if !base.is_empty() && !base.ends_with(&format!("{eol}{eol}")) {
        base.push_str(eol);
    }
    base.push_str(&build_block(ip, hosts, eol));
    base
}

/// True if this already-trimmed, non-empty, non-comment "<ip> host..." line
/// maps one of `wanted` (lowercased hostnames). Shared by `conflicts` (which
/// only reports) and `disable_conflicts` (which acts on it).
fn line_maps_host(trimmed: &str, wanted: &[String]) -> bool {
    // "<ip> host1 host2 ..." — any field after the first is a hostname.
    let mut fields = trimmed.split_whitespace();
    let _ip = fields.next();
    for name in fields {
        if name.starts_with('#') {
            break;
        }
        if wanted.contains(&name.to_ascii_lowercase()) {
            return true;
        }
    }
    false
}

/// Active (uncommented) lines outside our block that map one of `hosts`.
pub fn conflicts(content: &str, hosts: &[String]) -> Vec<String> {
    let stripped = strip_block(content);
    let wanted: Vec<String> = hosts.iter().map(|h| h.to_ascii_lowercase()).collect();

    stripped
        .lines()
        .map(str::trim)
        .filter(|text| !text.is_empty() && !text.starts_with('#'))
        .filter(|text| line_maps_host(text, &wanted))
        .map(String::from)
        .collect()
}

/// Splits a line (as yielded by `str::split_inclusive('\n')`) into its text
/// and the line ending it carried ("", "\n", or "\r\n").
fn split_line_ending(line: &str) -> (&str, &str) {
    if let Some(text) = line.strip_suffix("\r\n") {
        (text, "\r\n")
    } else if let Some(text) = line.strip_suffix('\n') {
        (text, "\n")
    } else {
        (line, "")
    }
}

/// Marks a line we disabled because it conflicted with one of our hostnames,
/// so `restore_conflicts` can put it back exactly as it was once our block
/// (and this override) are removed.
const DISABLED_PREFIX: &str = "# sp-launcher disabled: ";

/// Comment out active lines that map one of `hosts`, so our own redirect
/// always wins instead of losing to whichever entry happens to come first in
/// the file — Windows resolves a hostname using the first match it finds.
/// The original line is kept verbatim after `DISABLED_PREFIX` so
/// `restore_conflicts` can undo this exactly when the redirect is removed.
fn disable_conflicts(content: &str, hosts: &[String]) -> String {
    let wanted: Vec<String> = hosts.iter().map(|h| h.to_ascii_lowercase()).collect();
    let mut out = String::new();

    for line in content.split_inclusive('\n') {
        let (text, ending) = split_line_ending(line);
        let trimmed = text.trim();
        let is_conflict = !trimmed.is_empty() && !trimmed.starts_with('#') && line_maps_host(trimmed, &wanted);

        if is_conflict {
            out.push_str(DISABLED_PREFIX);
        }
        out.push_str(text);
        out.push_str(ending);
    }
    out
}

/// Undo `disable_conflicts`: strip the marker off every line that has it.
fn restore_conflicts(content: &str) -> String {
    content
        .split_inclusive('\n')
        .map(|line| {
            let (text, ending) = split_line_ending(line);
            let text = text.strip_prefix(DISABLED_PREFIX).unwrap_or(text);
            format!("{text}{ending}")
        })
        .collect()
}

fn has_disabled_conflicts(content: &str) -> bool {
    content.lines().any(|l| l.starts_with(DISABLED_PREFIX))
}

// -------------------------------------------------------------------- IO ---

fn backup_path(config_dir: &Path) -> PathBuf {
    config_dir.join("hosts.backup")
}

fn sentinel_path(config_dir: &Path) -> PathBuf {
    config_dir.join(SENTINEL)
}

pub fn read() -> Result<String> {
    let path = hosts_path();
    std::fs::read_to_string(&path).map_err(|e| {
        LauncherError::Message(format!("cannot read {}: {e}", path.display()))
    })
}

/// True when the process can actually modify the hosts file. This tests the
/// real thing rather than asking Windows whether the token is elevated —
/// antivirus and folder protection can block the write even when it is.
pub fn writable() -> bool {
    std::fs::OpenOptions::new()
        .append(true)
        .open(hosts_path())
        .is_ok()
}

fn write(content: &str) -> Result<()> {
    let path = hosts_path();
    // No temp-file-and-rename here: the hosts file has an ACL and replacing it
    // with a fresh file loses that. Writing in place keeps the ACL intact.
    std::fs::write(&path, content).map_err(|e| {
        LauncherError::Message(format!(
            "cannot write {} ({e}). The launcher needs to run as administrator.",
            path.display()
        ))
    })
}

pub fn flush_dns() {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("ipconfig")
            .arg("/flushdns")
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
}

pub fn status(hosts: &[String]) -> HostsStatus {
    let content = read().unwrap_or_default();
    HostsStatus {
        path: hosts_path().display().to_string(),
        writable: writable(),
        applied: has_block(&content),
        conflicts: conflicts(&content, hosts),
    }
}

pub fn apply(config_dir: &Path, ip: &str, hosts: &[String]) -> Result<()> {
    if hosts.is_empty() {
        return Ok(());
    }
    let content = read()?;

    // Back up the untouched file once, before the first edit we ever make.
    let backup = backup_path(config_dir);
    if !backup.exists() && !has_block(&content) {
        std::fs::create_dir_all(config_dir)?;
        std::fs::write(&backup, &content)?;
    }

    write(&with_block(&content, ip, hosts))?;
    // Pre-0.3.0 builds wrote a sentinel meaning "remove on next start". The
    // block is permanent now, so make sure no stale one is left.
    let _ = std::fs::remove_file(sentinel_path(config_dir));

    flush_dns();
    Ok(())
}

pub fn remove(config_dir: &Path) -> Result<()> {
    let content = read()?;
    let had_block = has_block(&content);
    let mut next = if had_block { strip_block(&content) } else { content };
    let had_disabled = has_disabled_conflicts(&next);
    if had_disabled {
        next = restore_conflicts(&next);
    }
    if had_block || had_disabled {
        write(&next)?;
        flush_dns();
    }
    let _ = std::fs::remove_file(sentinel_path(config_dir));
    Ok(())
}

/// Called at startup. Pre-0.3.0 builds removed the block when the game exited
/// and left a sentinel if they died first. The block is permanent now, so a
/// leftover sentinel is simply deleted; the block stays.
pub fn recover(config_dir: &Path) -> Option<String> {
    let _ = std::fs::remove_file(sentinel_path(config_dir));
    None
}

/// Is our block present with exactly the wanted lines, and nothing else in
/// the file overriding the same names? If so, nothing needs admin rights.
pub fn is_current(content: &str, ip: &str, hosts: &[String]) -> bool {
    if !has_block(content) || !conflicts(content, hosts).is_empty() {
        return false;
    }
    let mut inside = false;
    let mut got: Vec<String> = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t == BEGIN {
            inside = true;
            continue;
        }
        if t == END {
            break;
        }
        if inside && !t.is_empty() {
            got.push(t.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    let want: Vec<String> = hosts.iter().map(|h| format!("{ip} {h}")).collect();
    got == want
}

// ------------------------------------------------------------- elevation ---

/// Makes sure the block is in place. Returns immediately when it already is
/// (the normal case after the first start), writes it directly when the
/// process happens to be able to, and otherwise asks for ONE UAC prompt.
pub fn ensure(config_dir: &Path) -> Result<()> {
    let hosts = domains();
    if is_current(&read().unwrap_or_default(), BACKEND_IP, &hosts) {
        return Ok(());
    }
    if writable() {
        return apply(config_dir, BACKEND_IP, &hosts);
    }
    run_elevated("apply", config_dir)?;
    if is_current(&read().unwrap_or_default(), BACKEND_IP, &hosts) {
        Ok(())
    } else {
        Err(LauncherError::Message(
            "The hosts entries could not be written. An antivirus may be blocking changes to the hosts file."
                .into(),
        ))
    }
}

/// Takes the block out (Settings, "Remove entries"), elevating if needed.
pub fn remove_elevated(config_dir: &Path) -> Result<()> {
    if writable() {
        return remove(config_dir);
    }
    run_elevated("remove", config_dir)
}

/// Entry point of the elevated helper: `sp-launcher.exe --sp-hosts apply|remove <config dir>`.
/// Returns `None` when the process was not started as the helper.
pub fn helper_main() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 || args[1] != "--sp-hosts" {
        return None;
    }
    let dir = PathBuf::from(&args[3]);
    let res = match args[2].as_str() {
        "apply" => apply(&dir, BACKEND_IP, &domains()),
        "remove" => remove(&dir),
        _ => return Some(2),
    };
    Some(if res.is_ok() { 0 } else { 1 })
}

#[cfg(target_os = "windows")]
fn run_elevated(action: &str, config_dir: &Path) -> Result<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let dir = config_dir.display().to_string();
    if dir.contains('"') {
        return Err(LauncherError::Message("the config folder path contains a quote".into()));
    }
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let exe = wide(&std::env::current_exe()?.display().to_string());
    let verb = wide("runas");
    let params = wide(&format!("--sp-hosts {action} \"{dir}\""));

    // SAFETY: plain Win32 calls; every pointer outlives the call that uses it.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE;
        info.lpVerb = verb.as_ptr();
        info.lpFile = exe.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            // 1223 = ERROR_CANCELLED: the player said no to the UAC prompt.
            return Err(LauncherError::Message(if GetLastError() == 1223 {
                "The game needs a one-time admin permission to set up the connection to the server. \
                 Press Play again and choose Yes in the Windows prompt."
                    .into()
            } else {
                "Windows refused the admin prompt for the hosts file.".into()
            }));
        }
        if info.hProcess.is_null() {
            return Ok(());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code: u32 = 1;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if code != 0 {
            return Err(LauncherError::Message(
                "The hosts file could not be changed, even with admin rights. An antivirus may be blocking it.".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn run_elevated(_action: &str, _config_dir: &Path) -> Result<()> {
    Err(LauncherError::Message("elevation is only meaningful on Windows".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> Vec<String> {
        vec![
            "game.bravohotel.io".to_string(),
            "ui-lobby.bravohotel.io".to_string(),
        ]
    }

    const PLAIN: &str = "# Copyright\n127.0.0.1 localhost\n";

    #[test]
    fn adds_and_removes_cleanly() {
        let applied = with_block(PLAIN, "127.0.0.1", &hosts());
        assert!(has_block(&applied));
        assert!(applied.contains("127.0.0.1 game.bravohotel.io"));

        let removed = strip_block(&applied);
        assert!(!has_block(&removed));
        assert_eq!(removed, PLAIN, "the file returns to exactly its original text");
    }

    #[test]
    fn reapplying_replaces_rather_than_stacking() {
        let once = with_block(PLAIN, "127.0.0.1", &hosts());
        let twice = with_block(&once, "192.168.178.58", &hosts());
        assert_eq!(twice.matches(BEGIN).count(), 1, "exactly one block");
        assert!(twice.contains("192.168.178.58 game.bravohotel.io"));
        assert!(!twice.contains("127.0.0.1 game.bravohotel.io"));
    }

    #[test]
    fn preserves_crlf() {
        let crlf = "# c\r\n127.0.0.1 localhost\r\n";
        let applied = with_block(crlf, "127.0.0.1", &hosts());
        assert!(applied.contains("\r\n"));
        assert!(!applied.replace("\r\n", "").contains('\n'), "no bare LF introduced");
        assert_eq!(strip_block(&applied), crlf);
    }

    #[test]
    fn recovers_from_a_block_with_no_end_marker() {
        // What a kill -9 mid-write leaves behind.
        let truncated = format!("{PLAIN}{BEGIN}\n127.0.0.1 game.bravohotel.io\n");
        let removed = strip_block(&truncated);
        assert!(!removed.contains("bravohotel"));
        assert!(removed.starts_with("# Copyright"));
    }

    #[test]
    fn finds_hand_written_conflicts() {
        // His existing "# sp-backend" block is not ours, so we report it
        // instead of quietly deleting it.
        let manual = format!(
            "{PLAIN}# sp-backend start\n10.0.0.5 game.bravohotel.io\n# sp-backend end\n"
        );
        let found = conflicts(&manual, &hosts());
        assert_eq!(found, vec!["10.0.0.5 game.bravohotel.io"]);
    }

    #[test]
    fn ignores_commented_and_unrelated_lines() {
        let content = format!("{PLAIN}# 1.2.3.4 game.bravohotel.io\n5.6.7.8 example.com\n");
        assert!(conflicts(&content, &hosts()).is_empty());
    }

    #[test]
    fn our_own_block_is_not_a_conflict() {
        let applied = with_block(PLAIN, "127.0.0.1", &hosts());
        assert!(conflicts(&applied, &hosts()).is_empty());
    }

    #[test]
    fn applying_overrides_a_hand_written_conflict() {
        let manual = format!("{PLAIN}10.0.0.5 game.bravohotel.io\n");
        let applied = with_block(&manual, "127.0.0.1", &hosts());

        // The old entry is disabled, not deleted...
        assert!(applied.contains("# sp-launcher disabled: 10.0.0.5 game.bravohotel.io"));
        // ...our own block has the real value...
        assert!(applied.contains("127.0.0.1 game.bravohotel.io"));
        // ...and it no longer shows up as a live conflict.
        assert!(conflicts(&applied, &hosts()).is_empty());
    }

    #[test]
    fn removing_restores_an_overridden_conflict_exactly() {
        let manual = format!("{PLAIN}10.0.0.5 game.bravohotel.io\n");
        let applied = with_block(&manual, "127.0.0.1", &hosts());
        let restored = restore_conflicts(&strip_block(&applied));
        assert_eq!(restored, manual, "the disabled line comes back byte-for-byte");
    }
}
