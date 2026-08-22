import SwiftUI

@main
struct NetCheckApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @StateObject private var fetcher = StatusFetcher()

    var body: some Scene {
        WindowGroup("NetCheck") {
            ContentView(fetcher: fetcher)
                .frame(minWidth: 950, minHeight: 560)
        }
        .windowResizability(.contentSize)
    }
}
