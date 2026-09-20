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
//! Two things make this safe to do automatically. The original file is backed
//! up before the first edit, and a sentinel is written next to the config while
//! the block is applied — so if the launcher is killed, the next start finds
//! the sentinel and cleans up the block it left behind.
//!
//! The block editing is pure string work and unit-tested; only the file IO and
//! the DNS flush are Windows-specific.

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

    std::fs::write(
        sentinel_path(config_dir),
        serde_json::to_string(&Applied {
            ip: ip.to_string(),
            hosts: hosts.to_vec(),
        })
        .map_err(|e| LauncherError::Config(e.to_string()))?,
    )?;

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

/// Called at startup: if a previous run died with the block applied, take it
/// out before doing anything else.
pub fn recover(config_dir: &Path) -> Option<String> {
    let sentinel = sentinel_path(config_dir);
    if !sentinel.exists() {
        return None;
    }
    match remove(config_dir) {
        Ok(()) => Some("Removed hosts entries left behind by a previous run.".into()),
        Err(e) => Some(format!(
            "A previous run left hosts entries behind and they could not be removed: {e}"
        )),
    }
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
