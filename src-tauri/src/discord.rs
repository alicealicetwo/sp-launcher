//! Discord Rich Presence.
//!
//! All of this is best-effort and entirely optional: Discord may not be
//! running, may be closed halfway through a session, or may never have been
//! installed. None of that is the launcher's problem, so the IPC lives on its
//! own thread and every failure is swallowed — the worst case is that nobody
//! sees a status, never that a launch is held up. Talking to Discord on the
//! command path instead would risk blocking the game start behind a socket.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use discord_rich_presence::activity::{Activity, Assets, Button, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

/// The Discord application this presence belongs to. Its name is what players
/// see after "Playing", and `IMAGE_KEY` below names one of its uploaded Rich
/// Presence art assets.
///
/// Create an application at <https://discord.com/developers/applications> and
/// paste its Application ID here. Leaving this empty disables rich presence
/// altogether — the launcher behaves exactly as it did before — so an
/// unconfigured build is never a broken one.
pub const APP_ID: &str = "1541479671433011201";

/// Name of the image uploaded under the application's
/// Rich Presence → Art Assets. Ignored by Discord if it doesn't exist.
const IMAGE_KEY: &str = "sp_logo";

/// Shown as a button on the presence, so anyone who sees a player's status
/// can get to the server's Discord. Discord only renders buttons for *other*
/// people — a player never sees their own.
const DISCORD_URL: &str = "https://discord.gg/superpeopleofficial";

/// How long to wait before looking for Discord again when it isn't there.
/// Also the upper bound on how long a state change waits when Discord is
/// absent, which doesn't matter because nothing is displayed either way.
const RETRY: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    InLauncher,
    InGame,
}

impl State {
    fn details(self) -> &'static str {
        match self {
            State::InLauncher => "In the launcher",
            State::InGame => "In game",
        }
    }
}

/// Handle to the presence worker. Cloneable so the "game exited" task can
/// hold one, and a no-op when no `APP_ID` is configured.
#[derive(Clone)]
pub struct Presence {
    tx: Option<Sender<State>>,
}

impl Presence {
    pub fn start() -> Self {
        if APP_ID.is_empty() {
            return Self { tx: None };
        }

        let (tx, rx) = mpsc::channel::<State>();
        thread::spawn(move || {
            let mut client = DiscordIpcClient::new(APP_ID);
            let mut connected = false;
            let mut state = State::InLauncher;
            // Discord shows this as "elapsed", so it restarts whenever the
            // state does — "In game 00:05" should mean five minutes of game,
            // not five minutes since the launcher opened.
            let mut since = now_ms();
            let mut dirty = true;

            loop {
                match rx.recv_timeout(RETRY) {
                    Ok(next) => {
                        if next != state {
                            state = next;
                            since = now_ms();
                            dirty = true;
                        }
                    }
                    // Nothing changed; fall through so a missing Discord gets
                    // another chance to be found.
                    Err(RecvTimeoutError::Timeout) => {}
                    // Every sender is gone: the launcher is shutting down.
                    Err(RecvTimeoutError::Disconnected) => break,
                }

                if !connected {
                    // A fresh client each time: reconnecting a closed one
                    // would depend on its internal state being reusable.
                    client = DiscordIpcClient::new(APP_ID);
                    connected = client.connect().is_ok();
                    dirty = connected;
                }

                if connected && dirty {
                    if client.set_activity(activity(state, since)).is_err() {
                        // Discord went away mid-session — drop it and pick the
                        // connection back up on a later pass.
                        let _ = client.close();
                        connected = false;
                    } else {
                        dirty = false;
                    }
                }
            }

            if connected {
                let _ = client.clear_activity();
                let _ = client.close();
            }
        });

        Self { tx: Some(tx) }
    }

    /// Queue a state change. Never blocks, and does nothing when presence is
    /// disabled or the worker has stopped.
    pub fn set(&self, state: State) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(state);
        }
    }
}

fn activity<'a>(state: State, since: i64) -> Activity<'a> {
    Activity::new()
        .details(state.details())
        .assets(
            Assets::new()
                .large_image(IMAGE_KEY)
                .large_text("SUPER PEOPLE"),
        )
        .timestamps(Timestamps::new().start(since))
        .buttons(vec![Button::new("Join the Discord", DISCORD_URL)])
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
