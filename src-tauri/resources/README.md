# src-tauri/resources

## XAPOFX1_5.dll  — required before shipping a release

This is the no-Steam proxy from `sp-listen-patch/sp_proxy.cpp`. The launcher
embeds it with `include_bytes!` and writes it into the player's
`BravoHotelGame\Binaries\Win64\` before every launch, so the auto-updater
delivers it along with the launcher itself.

It is not optional. The launcher passes `-ServicePlatform=` , and without this
DLL intercepting the game's request for the Steam online subsystem, the client
waits forever for a Steam that is not running — the loading screen that never
ends.

To produce it:

    cd ..\sp-listen-patch
    build_sp_proxy.bat
    copy dist\XAPOFX1_5.dll ..\sp-launcher\src-tauri\resources\

`build.rs` checks for this file. If it is absent the launcher still compiles —
a fresh clone should not fail to build — but it prints a warning, ships without
the DLL, and logs `this launcher has no DLL bundled` at launch.
`tools\publish-update.ps1` refuses to build a release at all without it.

It is deliberately not in version control: it is a build artifact, it is
platform-specific, and a stale copy committed by accident is worse than no copy.
