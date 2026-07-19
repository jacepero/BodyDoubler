// Widget extension: Live Activity UI showing the current task + elapsed timer
// (display only, plan §8 Phase 4).

import WidgetKit
import SwiftUI
import ActivityKit

@main
struct BodyDoubleWidgetsBundle: WidgetBundle {
    var body: some Widget {
        CurrentTaskLiveActivity()
    }
}

struct CurrentTaskLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: CurrentTaskAttributes.self) { context in
            // Lock screen / banner
            HStack {
                Image(systemName: "scope")
                    .foregroundStyle(.blue)
                VStack(alignment: .leading, spacing: 2) {
                    Text(context.state.title)
                        .font(.headline)
                        .lineLimit(1)
                    Text(context.state.startedAt, style: .timer)
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
                Spacer()
            }
            .padding()
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Image(systemName: "scope").foregroundStyle(.blue)
                }
                DynamicIslandExpandedRegion(.center) {
                    Text(context.state.title).lineLimit(1)
                }
                DynamicIslandExpandedRegion(.trailing) {
                    Text(context.state.startedAt, style: .timer)
                        .monospacedDigit()
                }
            } compactLeading: {
                Image(systemName: "scope")
            } compactTrailing: {
                Text(context.state.startedAt, style: .timer)
                    .monospacedDigit()
                    .frame(maxWidth: 44)
            } minimal: {
                Image(systemName: "scope")
            }
        }
    }
}
