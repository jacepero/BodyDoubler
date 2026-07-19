// Shared between the app and the widget extension (compiled into both targets
// — see project.yml). Display-only Live Activity for the current task (plan §8).

import Foundation

#if canImport(ActivityKit)
import ActivityKit

struct CurrentTaskAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        var title: String
        var startedAt: Date
    }
}
#endif
