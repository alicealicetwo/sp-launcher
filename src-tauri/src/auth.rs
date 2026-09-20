//! Key-based login: redeeming a launcher key, and minting the one-time ticket
//! the game presents when it starts.
//!
//! THE SHAPE OF THE THING
//! ---------------------
//! A player gets a key from the Discord bot (`/authkey`) and enters it here
//! once. The key is a long-lived credential, so it is stored encrypted at rest
//! with Windows DPAPI and it never reaches the game process.
//!
//! At the moment Play is pressed the launcher swaps that key for a ONE-TIME
//! TICKET: valid for about a minute, dead after first use, bound to the
//! account. That ticket is what the game carries, and the backend resolves the
//! account from it.
//!
//! WHY IT IS BUILT THIS WAY
//! ------------------------
//! Everything on the player's machine is attacker-controlled: this launcher,
//! the game, the shim beside it. So an account id asserted by any of them is a
//! claim and not proof, and the backend must never trust one. It trusts only a
//! secret it issued itself. Hence: the permanent key stays here, and what
//! travels is short-lived, single-use and revocable.
//!
//! The consequence for this file: a stolen ticket is worth almost nothing, a
//! stolen key is worth everything. That is why the key is encrypted at rest and
//! is never logged, never put on a command line, and never handed to the game.

use serde::{Deserialize, Serialize};

use crate::error::{LauncherError, Result};

/// Where the backend lives. Fixed rather than configurable, exactly like
/// `news::FEED_URL`: it is this server's API, and a player pointing the
/// launcher somewhere else only breaks their own login.
///
/// The port must match `http.port` in the backend's config.json.
pub const AUTH_BASE_URL: &str = "http://64.226.112.204:8080/launcher/api";

/// How long a redeem/ticket call may take before we give up. Short on purpose:
/// this sits between the player pressing Play and the game starting, so a dead
/// backend has to fail fast and say so rather than look like a freeze.
const TIMEOUT_SECS: u64 = 10;

// ---------------------------------------------------------------- types ---

/// What the UI needs to know about the current sign-in. Deliberately does NOT
/// carry the key: nothing outside this module ever needs it, and the less it
/// travels the fewer places it can leak from.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AuthStatus {
    pub signed_in: bool,
    pub account_id: String,
    pub display_name: String,
    /// "active", "suspended" or "revoked" as the backend last reported it.
    pub status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RedeemOk {
    #[serde(default)]
    account_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct TicketOk {
    #[serde(default)]
    token: String,
    #[serde(default)]
    expires_in: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ApiError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    until: Option<String>,
}

/// A minted ticket, on its way to the game process.
#[derive(Debug, Clone)]
pub struct Ticket {
    pub token: String,
    pub expires_in: u64,
}

// --------------------------------------------------------------- errors ---

/// Turns a backend error code into something a player can act on.
///
/// The codes are the fixed vocabulary shared with the backend
/// (`routes/launcher.js`). An unknown one is passed through rather than
/// swallowed, so a future code is still visible instead of becoming
/// "something went wrong".
pub fn explain(code: &str, until: Option<&str>) -> String {
    match code {
        "KEY_INVALID" => "That key was not recognised. Check it for typos, or ask in Discord for a new one.".into(),
        "KEY_SUSPENDED" => match until {
            Some(t) if !t.is_empty() => format!("Your key is temporarily suspended (until {t}). Ask a Key Master in Discord."),
            _ => "Your key is temporarily suspended. Ask a Key Master in Discord.".into(),
        },
        "KEY_REVOKED" => "Your key has been revoked and cannot be used.".into(),
        "KEY_ALREADY_BOUND" => "That key is already in use on another PC. A Key Master can release it with /key unbind.".into(),
        "KEY_NOT_REDEEMED" => "This key has not been set up yet. Enter it again to finish signing in.".into(),
        "DEVICE_ID_REQUIRED" => "The launcher could not identify this installation. Restart it and try again.".into(),
        "RATE_LIMITED" => "Too many attempts. Wait a minute and try again.".into(),
        other => format!("Login failed ({other})."),
    }
}

// ------------------------------------------------------------ http calls ---

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(LauncherError::Http)
}

/// Shared shape: a 2xx carries the payload, anything else carries `{error}`.
/// Both are parsed, because a bare status code tells a player nothing.
async fn post<T: for<'de> Deserialize<'de>>(
    base: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<T> {
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let res = client()?.post(&url).json(&body).send().await.map_err(|e| {
        // A connection failure here is the single most common thing a player
        // will hit, so it gets a plain sentence rather than a reqwest dump.
        if e.is_timeout() {
            LauncherError::Message("The server did not respond. Try again in a moment.".into())
        } else if e.is_connect() {
            LauncherError::Message("Could not reach the server. Check your connection, or ask in Discord whether it is up.".into())
        } else {
            LauncherError::Http(e)
        }
    })?;

    let status = res.status();
    let text = res.text().await.unwrap_or_default();

    if status.is_success() {
        return serde_json::from_str::<T>(&text)
            .map_err(|e| LauncherError::Message(format!("The server sent something unexpected ({e}).")));
    }

    let err: ApiError = serde_json::from_str(&text).unwrap_or_default();
    let code = if err.error.is_empty() { format!("HTTP_{}", status.as_u16()) } else { err.error };
    Err(LauncherError::Message(explain(&code, err.until.as_deref())))
}

/// First-time sign-in. Binds the key to this installation and creates the
/// account on the backend if it does not exist yet.
pub async fn redeem(base: &str, key: &str, device_id: &str) -> Result<AuthStatus> {
    let key = normalize_key(key);
    if key.is_empty() {
        return Err(LauncherError::Message(
            "That does not look like a key. They look like SP-XXXX-XXXX-XXXX.".into(),
        ));
    }
    let ok: RedeemOk = post(
        base,
        "/keys/redeem",
        serde_json::json!({ "key": key, "device_id": device_id }),
    )
    .await?;

    Ok(AuthStatus {
        signed_in: true,
        account_id: ok.account_id,
        display_name: ok.display_name,
        status: if ok.status.is_empty() { "active".into() } else { ok.status },
    })
}

/// Called immediately before the game starts. The returned token is good for
/// one login and about a minute.
pub async fn mint_ticket(base: &str, key: &str, device_id: &str) -> Result<Ticket> {
    let ok: TicketOk = post(
        base,
        "/session/ticket",
        serde_json::json!({ "key": key, "device_id": device_id }),
    )
    .await?;

    if ok.token.is_empty() {
        return Err(LauncherError::Message("The server returned an empty login ticket.".into()));
    }
    Ok(Ticket { token: ok.token, expires_in: ok.expires_in })
}

// ------------------------------------------------------------ key format ---

/// Accepts what a player actually pastes: lower case, missing dashes, stray
/// spaces, a trailing newline from the Discord copy. Returns the canonical
/// `SP-XXXX-XXXX-XXXX`, or an empty string if it cannot be one.
pub fn normalize_key(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();

    if cleaned.len() != 14 || !cleaned.starts_with("SP") {
        return String::new();
    }
    let body = &cleaned[2..];
    if !body.chars().all(|c| c.is_ascii_alphanumeric()) {
        return String::new();
    }
    format!("SP-{}-{}-{}", &body[0..4], &body[4..8], &body[8..12])
}

// --------------------------------------------------------- key at rest ---

/// Hex rather than base64 so there is no dependency for it, and so a config
/// file is obviously-opaque instead of looking like readable text.
fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(windows)]
mod secret {
    //! DPAPI, user scope: only this Windows account on this machine can
    //! decrypt the blob. Copying config.v1.json to another PC gets an attacker
    //! nothing, which is the point.
    //!
    //! Signatures checked against windows-sys 0.59:
    //!   CryptProtectData(*const BLOB, PCWSTR, *const BLOB, *const c_void,
    //!                    *const CRYPTPROTECT_PROMPTSTRUCT, u32, *mut BLOB) -> BOOL
    //!   CryptUnprotectData(*const BLOB, *mut PWSTR, *const BLOB, *const c_void,
    //!                      *const CRYPTPROTECT_PROMPTSTRUCT, u32, *mut BLOB) -> BOOL
    //!   LocalFree(HLOCAL) -> HLOCAL, HLOCAL = *mut c_void
    //! Windows frees the output buffer's memory, not us, so every success path
    //! copies it out and then LocalFree's it.
    use crate::error::{LauncherError, Result};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    fn input_blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        // The API does not write through pdatain; the cast is only needed
        // because the struct field is typed *mut.
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    /// Copies the result out and hands the buffer back to Windows.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as *mut core::ffi::c_void);
        v
    }

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>> {
        let input = input_blob(plain);
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),      // szDataDescr
                std::ptr::null(),      // pOptionalEntropy
                std::ptr::null(),      // pvReserved
                std::ptr::null(),      // pPromptStruct
                0,
                &mut output,
            )
        };
        if ok == 0 {
            return Err(LauncherError::Message("Windows refused to encrypt the key.".into()));
        }
        Ok(unsafe { take(output) })
    }

    pub fn unprotect(sealed: &[u8]) -> Result<Vec<u8>> {
        let input = input_blob(sealed);
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),  // ppszDataDescr
                std::ptr::null(),      // pOptionalEntropy
                std::ptr::null(),      // pvReserved
                std::ptr::null(),      // pPromptStruct
                0,
                &mut output,
            )
        };
        if ok == 0 {
            // Wrong user, wrong machine, or a corrupted blob. All of them mean
            // the same thing to the player: sign in again.
            return Err(LauncherError::Message(
                "The stored key could not be read. Please enter it again.".into(),
            ));
        }
        Ok(unsafe { take(output) })
    }
}

#[cfg(not(windows))]
mod secret {
    //! The launcher ships for Windows only; this exists so the crate builds and
    //! its tests run on a developer machine. It is NOT encryption and does not
    //! pretend to be — it is a passthrough, and the comment is here so nobody
    //! ever mistakes a non-Windows build for something safe to hand to players.
    use crate::error::Result;
    pub fn protect(plain: &[u8]) -> Result<Vec<u8>> {
        Ok(plain.to_vec())
    }
    pub fn unprotect(blob: &[u8]) -> Result<Vec<u8>> {
        Ok(blob.to_vec())
    }
}

/// Encrypts a key for storage in the config file.
pub fn seal(key: &str) -> Result<String> {
    Ok(to_hex(&secret::protect(key.as_bytes())?))
}

/// Reverses `seal`. Any failure is reported as "sign in again" rather than
/// being papered over: a key that cannot be decrypted cannot be used.
pub fn unseal(stored: &str) -> Result<String> {
    let bytes = from_hex(stored)
        .ok_or_else(|| LauncherError::Message("The stored key is damaged. Please enter it again.".into()))?;
    let plain = secret::unprotect(&bytes)?;
    String::from_utf8(plain)
        .map_err(|_| LauncherError::Message("The stored key is damaged. Please enter it again.".into()))
}

// -------------------------------------------------------------- device id ---

/// Identifies this installation so a key can be bound to it.
///
/// Deliberately not cryptographic: it is a label, not a secret, and the
/// backend treats it as one. Uniqueness is all that is needed, so it is built
/// from the clock plus the address-space randomness the standard library
/// already has, which avoids adding a random-number dependency for a value
/// that is generated exactly once per install.
pub fn new_device_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let mut h = RandomState::new().build_hasher();
    h.write_u128(nanos);
    let a = h.finish();
    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(a);
    h2.write_u128(nanos);
    let b = h2.finish();

    format!("{a:016x}{b:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_what_a_player_actually_pastes() {
        let want = "SP-AB12-CD34-EF56";
        assert_eq!(normalize_key("SP-AB12-CD34-EF56"), want);
        assert_eq!(normalize_key("sp-ab12-cd34-ef56"), want);
        assert_eq!(normalize_key("SPAB12CD34EF56"), want);
        assert_eq!(normalize_key("  SP-AB12-CD34-EF56\n"), want);
        assert_eq!(normalize_key("SP AB12 CD34 EF56"), want);
    }

    #[test]
    fn rejects_things_that_are_not_keys() {
        assert_eq!(normalize_key(""), "");
        assert_eq!(normalize_key("hello"), "");
        assert_eq!(normalize_key("SP-AB12-CD34"), "");          // too short
        assert_eq!(normalize_key("SP-AB12-CD34-EF56-78"), "");  // too long
        assert_eq!(normalize_key("XX-AB12-CD34-EF56"), "");     // wrong prefix
    }

    #[test]
    fn hex_round_trips() {
        let data = b"\x00\x01\xfe\xff hello";
        assert_eq!(from_hex(&to_hex(data)).unwrap(), data);
        assert!(from_hex("abc").is_none());      // odd length
        assert!(from_hex("zz").is_none());       // not hex
    }

    #[test]
    fn a_sealed_key_comes_back_out() {
        let sealed = seal("SP-AB12-CD34-EF56").unwrap();
        assert_ne!(sealed, "SP-AB12-CD34-EF56", "must not be stored in the clear");
        assert_eq!(unseal(&sealed).unwrap(), "SP-AB12-CD34-EF56");
    }

    #[test]
    fn damaged_storage_asks_for_the_key_again_instead_of_panicking() {
        assert!(unseal("not hex at all").is_err());
        assert!(unseal("abc").is_err());
    }

    #[test]
    fn device_ids_are_unique_and_the_right_shape() {
        let a = new_device_id();
        let b = new_device_id();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn error_codes_become_sentences_a_player_can_act_on() {
        assert!(explain("KEY_INVALID", None).contains("typos"));
        assert!(explain("KEY_ALREADY_BOUND", None).contains("another PC"));
        assert!(explain("KEY_SUSPENDED", Some("2026-01-01T00:00:00Z")).contains("2026-01-01"));
        assert!(explain("KEY_SUSPENDED", None).contains("suspended"));
        // An unfamiliar code must still be visible, not swallowed.
        assert!(explain("SOME_NEW_CODE", None).contains("SOME_NEW_CODE"));
    }
}
