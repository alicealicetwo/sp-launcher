mod auth;
mod config;
mod discord;
mod engine_ini;
mod error;
mod game;
mod shim;
mod gateway;
pub mod hosts;
pub mod news;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use config::Config;
use error::{LauncherError, Result};

/// Everything the commands share. Plain mutexes: the contents are tiny and
/// only touched on user actions.
pub struct AppState {
    config_dir: PathBuf,
    config: Mutex<Config>,
    /// True while the game is running and our hosts block is in place.
    redirect_active: Arc<AtomicBool>,
    /// Set while the game is running, so `stop_game` knows what to kill.
    running_pid: Arc<Mutex<Option<u32>>>,
    /// Discord rich presence. A handle to a worker thread, so nothing here
    /// ever waits on Discord.
    discord: discord::Presence,
}

/// `Window` (handed to `on_window_event`) and `WebviewWindow` (handed back
/// by `get_webview_window`) have identical hide/show methods but no shared
/// trait for them, so this is the thin common interface that lets
/// `hide_to_tray_window`/`show_from_tray_window` work from either call site.
trait WindowLike {
    fn hide(&self) -> tauri::Result<()>;
    fn show(&self) -> tauri::Result<()>;
    fn set_focus(&self) -> tauri::Result<()>;
    fn set_skip_taskbar(&self, skip: bool) -> tauri::Result<()>;
}

impl<R: tauri::Runtime> WindowLike for tauri::Window<R> {
    fn hide(&self) -> tauri::Result<()> {
        tauri::Window::hide(self)
    }
    fn show(&self) -> tauri::Result<()> {
        tauri::Window::show(self)
    }
    fn set_focus(&self) -> tauri::Result<()> {
        tauri::Window::set_focus(self)
    }
    fn set_skip_taskbar(&self, skip: bool) -> tauri::Result<()> {
        tauri::Window::set_skip_taskbar(self, skip)
    }
}

impl<R: tauri::Runtime> WindowLike for tauri::WebviewWindow<R> {
    fn hide(&self) -> tauri::Result<()> {
        tauri::WebviewWindow::hide(self)
    }
    fn show(&self) -> tauri::Result<()> {
        tauri::WebviewWindow::show(self)
    }
    fn set_focus(&self) -> tauri::Result<()> {
        tauri::WebviewWindow::set_focus(self)
    }
    fn set_skip_taskbar(&self, skip: bool) -> tauri::Result<()> {
        tauri::WebviewWindow::set_skip_taskbar(self, skip)
    }
}

/// Hide the window and drop it from the taskbar, leaving the tray icon as
/// the only way back. Used by the minimize/close buttons and by a
/// CloseRequested from the OS (Alt+F4 etc.) alike, so there's one consistent
/// "hidden" state instead of minimize and close behaving differently.
fn hide_to_tray_window(window: &impl WindowLike) {
    let _ = window.hide();
    let _ = window.set_skip_taskbar(true);
}

/// Undo `hide_to_tray_window`: restore the taskbar entry, show, and focus.
fn show_from_tray_window(window: &impl WindowLike) {
    let _ = window.set_skip_taskbar(false);
    let _ = window.show();
    let _ = window.set_focus();
}

// ---------------------------------------------------------------- config ---

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> Config {
    state.config.lock().expect("config mutex").clone()
}

#[tauri::command]
fn set_config(state: State<'_, AppState>, cfg: Config) -> Result<()> {
    config::save(&state.config_dir, &cfg)?;
    *state.config.lock().expect("config mutex") = cfg;
    Ok(())
}

// --------------------------------------------------------------- install ---

#[tauri::command]
fn install_state(state: State<'_, AppState>) -> game::InstallState {
    let dir = state.config.lock().expect("config mutex").install_dir.clone();
    game::detect(&dir)
}

#[tauri::command]
fn open_install_dir(state: State<'_, AppState>) -> Result<()> {
    let dir = state.config.lock().expect("config mutex").install_dir.clone();
    if dir.is_empty() {
        return Err(LauncherError::Message("no install directory set".into()));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(&dir).spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open").arg(&dir).spawn()?;
    }
    Ok(())
}

// ------------------------------------------------------------------ news ---

#[tauri::command]
async fn fetch_news(state: State<'_, AppState>) -> Result<Vec<news::NewsItem>> {
    let _ = state;
    news::fetch(news::FEED_URL).await
}

// ----------------------------------------------------------------- hosts ---

#[tauri::command]
fn hosts_status(_state: State<'_, AppState>) -> hosts::HostsStatus {
    hosts::status(&hosts::domains())
}

#[tauri::command]
fn hosts_apply(state: State<'_, AppState>) -> Result<()> {
    hosts::apply(&state.config_dir, hosts::BACKEND_IP, &hosts::domains())?;
    state.redirect_active.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
fn hosts_remove(state: State<'_, AppState>) -> Result<()> {
    hosts::remove(&state.config_dir)?;
    state.redirect_active.store(false, Ordering::Relaxed);
    Ok(())
}

/// Relaunch the launcher with elevation. Uses PowerShell's Start-Process
/// rather than pulling in the Windows API crate for one call; the UAC prompt
/// is identical either way.
#[tauri::command]
fn relaunch_elevated(app: AppHandle) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe()?;
        let path = exe.display().to_string();
        if path.contains('\'') {
            return Err(LauncherError::Message(
                "cannot elevate: the launcher path contains a quote".into(),
            ));
        }
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                &format!("Start-Process -FilePath '{path}' -Verb RunAs"),
            ])
            .spawn()
            .map_err(|e| LauncherError::Message(format!("could not request elevation: {e}")))?;
        app.exit(0);
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err(LauncherError::Message(
            "elevation is only meaningful on Windows".into(),
        ))
    }
}

// ------------------------------------------------------------------ tray ---

/// Called by the minimize and close title-bar buttons: send the window to
/// the tray instead of the taskbar or actually quitting.
#[tauri::command]
fn hide_to_tray(app: AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        hide_to_tray_window(&window);
    }
    Ok(())
}

// ------------------------------------------------------------------ auth ---
//
// The key never leaves this file except to go to the backend. It is not
// returned to the frontend, not logged, and not passed to the game -- what the
// game gets is a one-time ticket minted at launch.

/// Makes sure this installation has an id, persisting it the first time.
fn ensure_device_id(state: &State<'_, AppState>) -> Result<String> {
    {
        let cfg = state.config.lock().expect("config mutex");
        if !cfg.device_id.is_empty() {
            return Ok(cfg.device_id.clone());
        }
    }
    let id = auth::new_device_id();
    {
        let mut cfg = state.config.lock().expect("config mutex");
        cfg.device_id = id.clone();
        config::save(&state.config_dir, &cfg)?;
    }
    Ok(id)
}

#[tauri::command]
fn auth_status(state: State<'_, AppState>) -> Result<auth::AuthStatus> {
    let cfg = state.config.lock().expect("config mutex");
    Ok(auth::AuthStatus {
        signed_in: !cfg.auth_key_sealed.is_empty(),
        account_id: cfg.account_id.clone(),
        display_name: cfg.display_name.clone(),
        status: cfg.key_status.clone(),
    })
}

#[tauri::command]
async fn redeem_key(state: State<'_, AppState>, key: String) -> Result<auth::AuthStatus> {
    let device_id = ensure_device_id(&state)?;
    let normalized = auth::normalize_key(&key);

    let status = auth::redeem(auth::AUTH_BASE_URL, &normalized, &device_id).await?;

    // Only store the key once the backend has accepted it, so a typo never
    // leaves a dead key sitting in the config.
    let sealed = auth::seal(&normalized)?;
    {
        let mut cfg = state.config.lock().expect("config mutex");
        cfg.auth_key_sealed = sealed;
        cfg.account_id = status.account_id.clone();
        cfg.display_name = status.display_name.clone();
        cfg.key_status = status.status.clone();
        config::save(&state.config_dir, &cfg)?;
    }
    Ok(status)
}

#[tauri::command]
fn sign_out(state: State<'_, AppState>) -> Result<()> {
    let mut cfg = state.config.lock().expect("config mutex");
    cfg.auth_key_sealed.clear();
    cfg.account_id.clear();
    cfg.display_name.clear();
    cfg.key_status.clear();
    // device_id deliberately survives: it identifies the installation, not the
    // player, and keeping it means signing back in is not treated as a move to
    // a new PC.
    config::save(&state.config_dir, &cfg)?;
    Ok(())
}

// ---------------------------------------------------------------- launch ---

#[derive(Serialize)]
struct LaunchResult {
    pid: u32,
    redirected: bool,
}

#[tauri::command]
async fn launch_game(
    app: AppHandle,
    state: State<'_, AppState>,
    server: Option<String>,
) -> Result<LaunchResult> {
    let cfg = state.config.lock().expect("config mutex").clone();
    let config_dir = state.config_dir.clone();

    // Mint the login ticket FIRST, before anything with a side effect.
    //
    // It is the step most likely to fail -- no key yet, key suspended, backend
    // down -- and failing here leaves the machine completely untouched: no
    // Engine.ini edit, no hosts entries, no process. It also means the game is
    // never started in a state where it cannot log in.
    let mut env: Vec<(String, String)> = Vec::new();
    if cfg.auth_key_sealed.is_empty() {
        return Err(LauncherError::Message(
            "You are not signed in. Enter your launcher key first -- get one with /authkey in Discord.".into(),
        ));
    }
    {
        let key = auth::unseal(&cfg.auth_key_sealed)?;
        let ticket = auth::mint_ticket(auth::AUTH_BASE_URL, &key, &cfg.device_id).await?;
        if cfg.debug_logging {
            // The lifetime, never the token. A ticket in a log file is a ticket
            // someone else can use for the next minute.
            eprintln!("[auth] login ticket minted, valid for {}s", ticket.expires_in);
        }
        // Environment, not argv: see the note on LaunchSpec::env.
        env.push(("SP_AUTH_TICKET".into(), ticket.token));
    }

    // `n.VerifyPeer=False` has to be in the player's Engine.ini before the
    // client starts reading config. Done before the hosts redirect so that a
    // failure here leaves the system completely untouched — there is nothing
    // to roll back yet at this point.
    // The no-Steam DLL, before anything starts the game: Windows holds a loaded
    // DLL open, so this is the only moment it can be written. Placed before the
    // hosts redirect for the same reason engine_ini is -- a failure here leaves
    // the machine untouched.
    match shim::apply(&cfg.install_dir) {
        Ok(shim::Applied::NotBundled) => {
            eprintln!("[shim] this launcher has no DLL bundled -- the game needs XAPOFX1_5.dll placed by hand");
        }
        Ok(what) => {
            if cfg.debug_logging {
                eprintln!("[shim] no-Steam DLL: {what:?}");
            }
        }
        Err(e) => return Err(e),
    }

    engine_ini::apply()?;

    // Redirect first: the game reads the hostnames on startup, so the entries
    // have to be in place before the process exists, not just before it
    // connects.
    let redirected = if cfg.hosts_redirect {
        if !hosts::writable() {
            return Err(LauncherError::Message(
                "The hosts file is not writable. Restart the launcher as administrator, \
                 or turn off the hosts redirect in Settings."
                    .into(),
            ));
        }
        hosts::apply(&config_dir, hosts::BACKEND_IP, &hosts::domains())?;
        state.redirect_active.store(true, Ordering::Relaxed);
        true
    } else {
        false
    };

    let child = game::launch(game::LaunchSpec {
        install_dir: &cfg.install_dir,
        server: server.as_deref(),
        user_args: &cfg.launch_args,
        env: &env,
    })
    .inspect_err(|_| {
        // The game never started, so take the entries straight back out.
        if redirected {
            let _ = hosts::remove(&config_dir);
            state.redirect_active.store(false, Ordering::Relaxed);
        }
    })?;

    let pid = child.id();
    *state.running_pid.lock().expect("pid mutex") = Some(pid);
    state.discord.set(discord::State::InGame);

    // The launcher used to hide itself here and only reappear when the game
    // exited. Now it stays open — the Play tab swaps its button for "Close
    // Game" instead — and `close_on_launch` just means "send it to the tray
    // instead", which the tray icon can bring back at any time.
    if cfg.close_on_launch {
        if let Some(window) = app.get_webview_window("main") {
            hide_to_tray_window(&window);
        }
    }

    // Wait for the game in the background and undo the redirect the moment it
    // exits, so the hosts file is only modified while the game is actually up.
    // This also fires when `stop_game` kills the process, so that command
    // doesn't need to duplicate any of this cleanup itself.
    {
        let app = app.clone();
        let redirect_active = state.redirect_active.clone();
        let running_pid = state.running_pid.clone();
        let presence = state.discord.clone();
        let mut child = child;
        tauri::async_runtime::spawn_blocking(move || {
            let status = child.wait();
            *running_pid.lock().expect("pid mutex") = None;
            presence.set(discord::State::InLauncher);
            if redirected {
                match hosts::remove(&config_dir) {
                    Ok(()) => redirect_active.store(false, Ordering::Relaxed),
                    Err(e) => {
                        let _ = app.emit("hosts:error", e.to_string());
                    }
                }
            }
            let code = status.ok().and_then(|s| s.code());
            let _ = app.emit("game:exited", code);
            if let Some(window) = app.get_webview_window("main") {
                show_from_tray_window(&window);
            }
        });
    }

    Ok(LaunchResult { pid, redirected })
}

/// Kills the running game. Doesn't touch the hosts redirect or emit
/// `game:exited` itself — the `wait()` in `launch_game`'s background task
/// notices the process die and does all of that, the same as if the game
/// had exited on its own.
#[tauri::command]
fn stop_game(state: State<'_, AppState>) -> Result<()> {
    let pid = *state.running_pid.lock().expect("pid mutex");
    let Some(pid) = pid else {
        return Err(LauncherError::Message("No game is running".into()));
    };

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| LauncherError::Message(format!("could not stop the game: {e}")))?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = pid;
        Err(LauncherError::Message(
            "stopping the game is only implemented on Windows".into(),
        ))
    }
}

// ------------------------------------------------------------------ entry ---

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Registered first, as the plugin requires: a second launch hands its
        // arguments to the running instance and exits. Since closing the
        // window only hides it to the tray, "already running" is usually
        // invisible — so bring that window back rather than doing nothing,
        // which would look like the launcher refusing to start.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                show_from_tray_window(&window);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let cfg = config::load(&config_dir);

            // If a previous run was killed with the redirect applied, clean it
            // up before the user can do anything else.
            if let Some(note) = hosts::recover(&config_dir) {
                let _ = app.handle().emit("hosts:recovered", note);
            }

            app.manage(AppState {
                config_dir,
                config: Mutex::new(cfg),
                redirect_active: Arc::new(AtomicBool::new(false)),
                running_pid: Arc::new(Mutex::new(None)),
                discord: discord::Presence::start(),
            });

            // Tray icon: reuses the app's own bundled icon rather than
            // shipping a second asset. "Open" undoes hide_to_tray_window;
            // "Quit" is now the only real way to end the process, since
            // closing the window itself just hides it.
            let show_item = MenuItem::with_id(app, "show", "Open SP Launcher", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            // Not `default_window_icon()`: the app icon keeps the whole mark,
            // wings included, which is 2.19:1 — in a square tray cell that
            // leaves it spanning the full width but under half the height, so
            // it reads as tiny next to everything else in the tray. tray.png
            // is the same artwork cropped to a squarer region, so it fills the
            // cell properly. Only the tray uses it; the installer, taskbar and
            // window icon still get the full logo.
            let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;

            TrayIconBuilder::new()
                .icon(tray_icon)
                .tooltip("SP Launcher")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            show_from_tray_window(&window);
                        }
                    }
                    "quit" => {
                        // Don't rely on the Destroyed handler firing before
                        // app.exit() tears things down — clean up explicitly.
                        if let Some(state) = app.try_state::<AppState>() {
                            if state.redirect_active.load(Ordering::Relaxed) {
                                let _ = hosts::remove(&state.config_dir);
                            }
                        }
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            show_from_tray_window(&window);
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The X button and Alt+F4 both raise this. Hide to the tray
            // instead of letting the app quit — Destroyed (below) is now
            // reserved for an actual exit via the tray's Quit item.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                hide_to_tray_window(window);
            }
            // Closing the window must not leave the hosts file edited.
            WindowEvent::Destroyed => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    if state.redirect_active.load(Ordering::Relaxed) {
                        let _ = hosts::remove(&state.config_dir);
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            install_state,
            open_install_dir,
            fetch_news,
            hosts_status,
            hosts_apply,
            hosts_remove,
            relaunch_elevated,
            hide_to_tray,
            launch_game,
            stop_game,
            auth_status,
            redeem_key,
            sign_out,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
