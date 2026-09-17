fn main() {
    // Brand the exe (taskbar + explorer icon) with the app icon. Best-effort:
    // a missing rc toolchain must not break the build.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../icons/icon.ico");
        res.set("ProductName", "Orrery Updater");
        res.set("FileDescription", "Orrery Update");
        // CRITICAL: a manifest-less exe whose name contains "updater" trips
        // Windows' installer-detection heuristic and demands ELEVATION (UAC) on
        // launch — exactly what this whole design avoids. asInvoker disables
        // that; PerMonitorV2 keeps the webview crisp on scaled displays.
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>"#,
        );
        if let Err(e) = res.compile() {
            // Without the manifest the exe would UAC-prompt — fail loudly.
            panic!("winresource compile failed (manifest is required): {e}");
        }
    }
}
