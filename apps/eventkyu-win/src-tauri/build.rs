fn main() {
    // tauri.conf.json embeds the WebView2 bootstrapper for connected installs.
    // Offline show builds must switch windows.webviewInstallMode.type to
    // "offlineInstaller" and ship the matching fixed Evergreen installer.
    tauri_build::build()
}
