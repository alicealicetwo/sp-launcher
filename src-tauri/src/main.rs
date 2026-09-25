// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Elevated helper mode (`--sp-hosts ...`): edit the hosts file and exit
    // before any window, tray icon or single-instance check exists.
    if let Some(code) = sp_launcher_lib::hosts::helper_main() {
        std::process::exit(code);
    }
    sp_launcher_lib::run()
}
