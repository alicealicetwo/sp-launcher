mod config;
pub mod download;
mod error;
mod game;
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
    cancel: Arc<AtomicBool>,
    /// True while the game is running and our hosts block is in place.
    redirect_active: Arc<AtomicBool>,
    /// Set while the game is running, so `stop_game` knows what to kill.
    running_pid: Arc<Mutex<Option<u32>>>,
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

/// Bridges the download module's events onto Tauri's IPC.
struct AppEvents(AppHandle);

impl download::Events for AppEvents {
    fn file(&self, state: download::FileState) {
        let _ = self.0.emit("download:file", state);
    }
    fn progress(&self, progress: download::Progress) {
        let _ = self.0.emit("download:progress", progress);
    }
    fn complete(&self, version: String) {
        let _ = self.0.emit("download:complete", version);
    }
    fn cancelled(&self) {
        let _ = self.0.emit("download:cancelled", ());
    }
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

// -------------------------------------------------------------- download ---

#[tauri::command]
async fn start_download(app: AppHandle, state: State<'_, AppState>) -> Result<()> {
    let (manifest_url, install_dir, threads) = {
        let cfg = state.config.lock().expect("config mutex");
        (
            cfg.manifest_url.clone(),
            cfg.install_dir.clone(),
            cfg.download_threads,
        )
    };

    if install_dir.is_empty() {
        return Err(LauncherError::Message("pick an install folder first".into()));
    }

    state.cancel.store(false, Ordering::Relaxed);
    let cancel = state.cancel.clone();

    let manifest: download::Manifest = reqwest::get(&manifest_url)
        .await?
        .error_for_status()?
        .json()
        .await?;

    download::run(
        Arc::new(AppEvents(app)),
        manifest,
        PathBuf::from(install_dir),
        cancel,
        threads as usize,
    )
    .await
}

#[tauri::command]
fn cancel_download(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::Relaxed);
}

// ------------------------------------------------------------------ news ---

#[tauri::command]
async fn fetch_news(state: State<'_, AppState>) -> Result<Vec<news::NewsItem>> {
    let news_url = state.config.lock().expect("config mutex").news_url.clone();
    news::fetch(&news_url).await
}

// ----------------------------------------------------------------- hosts ---

#[tauri::command]
fn hosts_status(state: State<'_, AppState>) -> hosts::HostsStatus {
    let domains = state
        .config
        .lock()
        .expect("config mutex")
        .hosts_domains
        .clone();
    hosts::status(&domains)
}

#[tauri::command]
fn hosts_apply(state: State<'_, AppState>) -> Result<()> {
    let cfg = state.config.lock().expect("config mutex").clone();
    hosts::apply(&state.config_dir, &cfg.backend_ip, &cfg.hosts_domains)?;
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

    // Redirect first: the game reads the hostnames on startup, so the entries
    // have to be in place before the process exists, not just before it
    // connects.
    let redirected = if cfg.hosts_redirect && !cfg.hosts_domains.is_empty() {
        if !hosts::writable() {
            return Err(LauncherError::Message(
                "The hosts file is not writable. Restart the launcher as administrator, \
                 or turn off the hosts redirect in Settings."
                    .into(),
            ));
        }
        hosts::apply(&config_dir, &cfg.backend_ip, &cfg.hosts_domains)?;
        state.redirect_active.store(true, Ordering::Relaxed);
        true
    } else {
        false
    };

    let child = game::launch(game::LaunchSpec {
        install_dir: &cfg.install_dir,
        server: server.as_deref(),
        user_args: &cfg.launch_args,
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
        let mut child = child;
        tauri::async_runtime::spawn_blocking(move || {
            let status = child.wait();
            *running_pid.lock().expect("pid mutex") = None;
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
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
                cancel: Arc::new(AtomicBool::new(false)),
                redirect_active: Arc::new(AtomicBool::new(false)),
                running_pid: Arc::new(Mutex::new(None)),
            });

            // Tray icon: reuses the app's own bundled icon rather than
            // shipping a second asset. "Open" undoes hide_to_tray_window;
            // "Quit" is now the only real way to end the process, since
            // closing the window itself just hides it.
            let show_item = MenuItem::with_id(app, "show", "Open SP Launcher", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("app icon is bundled"))
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
            start_download,
            cancel_download,
            fetch_news,
            hosts_status,
            hosts_apply,
            hosts_remove,
            relaunch_elevated,
            hide_to_tray,
            launch_game,
            stop_game,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
