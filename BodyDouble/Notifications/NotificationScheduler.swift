// Check-in scheduling (plan §7): local notifications with a text-input action.
// iOS gives us no persistent background process, so a CHAIN of the next 8
// check-ins is scheduled up front; any app-open or reply reschedules from now.
// Quiet hours shift a slot to the end of the quiet window.

import Foundation
import UserNotifications

@MainActor
final class NotificationScheduler: ObservableObject {
    static let categoryId = "CHECKIN"
    static let replyActionId = "CHECKIN_REPLY"
    private static let chainPrefix = "checkin.chain."
    private static let oneOffPrefix = "checkin.oneoff."
    private static let chainLength = 8

    private let store: Store
    private let settings: AppSettings

    @Published private(set) var authorized = false

    init(store: Store, settings: AppSettings) {
        self.store = store
        self.settings = settings
    }

    // MARK: - Setup

    func bootstrap() async {
        let center = UNUserNotificationCenter.current()

        let reply = UNTextInputNotificationAction(
            identifier: Self.replyActionId,
            title: "Reply",
            options: [],
            textInputButtonTitle: "Send",
            textInputPlaceholder: "What are you up to?")
        let category = UNNotificationCategory(
            identifier: Self.categoryId,
            actions: [reply],
            intentIdentifiers: [],
            options: [])
        center.setNotificationCategories([category])

        if !settings.didRequestNotifications {
            settings.didRequestNotifications = true
            authorized = (try? await center.requestAuthorization(
                options: [.alert, .sound, .badge])) ?? false
        } else {
            let s = await center.notificationSettings()
            authorized = s.authorizationStatus == .authorized
                || s.authorizationStatus == .provisional
        }
        if authorized {
            rescheduleChain()
        }
    }

    // MARK: - Scheduling

    /// Cancels pending check-ins and schedules the next chain from `now`
    /// (plan §7 step 3: opening the app resets the cadence).
    func rescheduleChain(from now: Date = Date()) {
        let center = UNUserNotificationCenter.current()
        center.getPendingNotificationRequests { [weak self] pending in
            let ids = pending.map(\.identifier)
                .filter { $0.hasPrefix(Self.chainPrefix) }
            center.removePendingNotificationRequests(withIdentifiers: ids)
            Task { @MainActor in self?.scheduleChain(from: now) }
        }
    }

    private func scheduleChain(from now: Date) {
        try? store.clearUnfiredCheckins()
        let interval = TimeInterval(max(5, settings.checkinIntervalMinutes) * 60)
        var fireDate = now
        for i in 0..<Self.chainLength {
            fireDate = adjustedForQuietHours(fireDate.addingTimeInterval(interval))
            schedule(at: fireDate, identifier: Self.chainPrefix + String(i), from: now)
        }
    }

    /// Model-requested one-off (schedule_checkin tool).
    func scheduleOneOff(minutesFromNow: Int) {
        let fireDate = Date().addingTimeInterval(TimeInterval(minutesFromNow * 60))
        schedule(at: adjustedForQuietHours(fireDate),
                 identifier: Self.oneOffPrefix + UUID().uuidString,
                 from: Date())
    }

    private func schedule(at fireDate: Date, identifier: String, from now: Date) {
        let content = UNMutableNotificationContent()
        content.title = "Check-in"
        content.body = "What are you up to?"
        content.categoryIdentifier = Self.categoryId
        content.sound = .default

        let seconds = max(60, fireDate.timeIntervalSince(now))
        let trigger = UNTimeIntervalNotificationTrigger(timeInterval: seconds, repeats: false)
        let request = UNNotificationRequest(identifier: identifier,
                                            content: content, trigger: trigger)
        UNUserNotificationCenter.current().add(request)
        try? store.recordScheduledCheckin(for: now.addingTimeInterval(seconds))
    }

    // MARK: - Quiet hours (plan §7 step 4)

    /// If `date` lands inside the quiet window, push it to the window's end.
    func adjustedForQuietHours(_ date: Date, calendar: Calendar = .current) -> Date {
        guard settings.quietHoursEnabled, isQuiet(date, calendar: calendar) else { return date }
        let end = settings.quietEndHour
        var target = calendar.date(bySettingHour: end, minute: 0, second: 0, of: date) ?? date
        if target <= date {
            target = calendar.date(byAdding: .day, value: 1, to: target) ?? target
        }
        return target
    }

    func isQuiet(_ date: Date, calendar: Calendar = .current) -> Bool {
        guard settings.quietHoursEnabled else { return false }
        let hour = calendar.component(.hour, from: date)
        let start = settings.quietStartHour
        let end = settings.quietEndHour
        if start == end { return false }
        if start < end {
            return hour >= start && hour < end
        }
        // Window wraps midnight (e.g. 22 -> 8).
        return hour >= start || hour < end
    }
}
