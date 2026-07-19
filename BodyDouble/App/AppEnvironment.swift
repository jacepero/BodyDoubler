// Composition root: builds and wires every subsystem (plan §3 architecture).

import Foundation
import UserNotifications

@MainActor
final class AppEnvironment: ObservableObject {
    let store: Store
    let settings: AppSettings
    let engine: InferenceEngine
    let downloader: ModelDownloader
    let speech: SpeechIO
    let scheduler: NotificationScheduler
    let coordinator: SessionCoordinator
    let notificationDelegate: NotificationDelegate
    let liveActivity: LiveActivityManager

    private var didBootstrap = false

    init() {
        // A failed store open is unrecoverable — fall back to in-memory so the
        // app at least launches (and surfaces the problem) instead of crashing.
        let store = (try? Store.makeDefault()) ?? (try! Store.inMemory())
        self.store = store

        let settings = AppSettings(store: store)
        self.settings = settings

        engine = InferenceEngine()
        downloader = ModelDownloader()
        speech = SpeechIO()
        scheduler = NotificationScheduler(store: store, settings: settings)
        liveActivity = LiveActivityManager()

        coordinator = SessionCoordinator(store: store, settings: settings,
                                         engine: engine, speech: speech)
        coordinator.scheduler = scheduler
        coordinator.liveActivity = liveActivity

        notificationDelegate = NotificationDelegate()
        notificationDelegate.onReply = { [coordinator] text in
            coordinator.handleNotificationReply(text)
        }
        UNUserNotificationCenter.current().delegate = notificationDelegate
    }

    /// Called every time the app becomes active (plan §7 step 3): reset the
    /// check-in cadence and run any turn deferred from a background reply.
    func onForeground() {
        Task {
            if !didBootstrap {
                didBootstrap = true
                await scheduler.bootstrap()
            } else {
                scheduler.rescheduleChain()
            }
            await coordinator.processPendingTurn()
            liveActivity.refresh(from: store)
        }
    }
}
