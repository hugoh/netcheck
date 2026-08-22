import AppKit

/// Without a real .app bundle (e.g. running via `swift run`), macOS doesn't
/// automatically foreground the window — the process launches but nothing
/// visibly appears. Explicitly activating fixes that.
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }
}
