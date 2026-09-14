fn main() {
    #[cfg(target_os = "windows")]
    {
        // Always run elevated: the hosts-file redirect needs administrator
        // rights, and asking for elevation only when the toggle is on meant
        // a relaunch in the middle of using the app. This embeds a manifest
        // so Windows prompts for UAC once, at every launch, up front.
        //
        // `app_manifest` REPLACES tauri's default manifest wholesale, not
        // merges with it — and the default one exists solely to declare a
        // dependency on comctl32.dll v6 (the "Common-Controls" assembly).
        // Dropping that dependency, as an earlier version of this manifest
        // did, makes Windows fall back to the ancient v5 comctl32.dll, which
        // is missing exports like TaskDialogIndirect that Tauri/WebView2
        // rely on — and the app then fails to start at all with
        // STATUS_ENTRYPOINT_NOT_FOUND. So both pieces have to live in the
        // one manifest below.
        let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;
        let windows = tauri_build::WindowsAttributes::new().app_manifest(manifest);
        let attrs = tauri_build::Attributes::new().windows_attributes(windows);
        tauri_build::try_build(attrs).expect("failed to run tauri-build");
    }
    #[cfg(not(target_os = "windows"))]
    {
        tauri_build::build();
    }
}
