// Starts/updates/ends the display-only current-task Live Activity (plan §8
// Phase 4). Purely cosmetic — failure is always silent.

import Foundation

#if canImport(ActivityKit)
import ActivityKit
#endif

@MainActor
final class LiveActivityManager {

    func refresh(from store: Store) {
        #if canImport(ActivityKit)
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }

        let current = (try? store.currentTask()) ?? nil
        let existing = Activity<CurrentTaskAttributes>.activities.first

        guard let current, let startedAt = current.updatedDate else {
            // No current task -> end any running activity.
            if let existing {
                Task { await existing.end(nil, dismissalPolicy: .immediate) }
            }
            return
        }

        let state = CurrentTaskAttributes.ContentState(title: current.title,
                                                       startedAt: startedAt)
        if let existing {
            Task { await existing.update(ActivityContent(state: state, staleDate: nil)) }
        } else {
            _ = try? Activity.request(
                attributes: CurrentTaskAttributes(),
                content: ActivityContent(state: state, staleDate: nil))
        }
        #endif
    }
}
