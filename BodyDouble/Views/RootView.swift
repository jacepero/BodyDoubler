import SwiftUI

struct RootView: View {
    var body: some View {
        TabView {
            ChatView()
                .tabItem { Label("Now", systemImage: "bubble.left.and.bubble.right") }
            JournalTimelineView()
                .tabItem { Label("Timeline", systemImage: "clock") }
            TasksView()
                .tabItem { Label("Tasks", systemImage: "checklist") }
            SettingsView()
                .tabItem { Label("Settings", systemImage: "gearshape") }
        }
    }
}
