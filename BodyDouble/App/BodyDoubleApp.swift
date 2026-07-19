import SwiftUI

@main
struct BodyDoubleApp: App {
    @StateObject private var env = AppEnvironment()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(env)
                .environmentObject(env.coordinator)
                .environmentObject(env.settings)
                .environmentObject(env.downloader)
                .environmentObject(env.speech)
                .environmentObject(env.scheduler)
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active {
                env.onForeground()
            }
        }
    }
}
