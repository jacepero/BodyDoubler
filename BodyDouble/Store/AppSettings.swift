// Typed, observable wrapper over the app_state key/value table (plan §4).
// Every property persists on write, so settings survive relaunch.

import Foundation
import Combine

@MainActor
final class AppSettings: ObservableObject {
    enum Keys {
        static let checkinInterval = "checkin_interval_min"
        static let quietEnabled = "quiet_hours_enabled"
        static let quietStart = "quiet_start_hour"
        static let quietEnd = "quiet_end_hour"
        static let voiceId = "voice_id"
        static let ttsEnabled = "tts_enabled"
        static let modelTier = "model_tier"
        static let didRequestNotifications = "did_request_notifications"
        static let pendingEntryId = "pending_turn_entry_id"
    }

    private let store: Store

    @Published var checkinIntervalMinutes: Int {
        didSet { store.setStateValue(Keys.checkinInterval, String(checkinIntervalMinutes)) }
    }
    @Published var quietHoursEnabled: Bool {
        didSet { store.setStateValue(Keys.quietEnabled, quietHoursEnabled ? "1" : "0") }
    }
    @Published var quietStartHour: Int {
        didSet { store.setStateValue(Keys.quietStart, String(quietStartHour)) }
    }
    @Published var quietEndHour: Int {
        didSet { store.setStateValue(Keys.quietEnd, String(quietEndHour)) }
    }
    @Published var voiceIdentifier: String? {
        didSet { store.setStateValue(Keys.voiceId, voiceIdentifier) }
    }
    @Published var ttsEnabled: Bool {
        didSet { store.setStateValue(Keys.ttsEnabled, ttsEnabled ? "1" : "0") }
    }
    @Published var modelTier: ModelTier {
        didSet { store.setStateValue(Keys.modelTier, modelTier.rawValue) }
    }

    init(store: Store) {
        self.store = store
        checkinIntervalMinutes = Int(store.stateValue(Keys.checkinInterval) ?? "") ?? 45
        quietHoursEnabled = (store.stateValue(Keys.quietEnabled) ?? "1") == "1"
        quietStartHour = Int(store.stateValue(Keys.quietStart) ?? "") ?? 22
        quietEndHour = Int(store.stateValue(Keys.quietEnd) ?? "") ?? 8
        voiceIdentifier = store.stateValue(Keys.voiceId)
        ttsEnabled = (store.stateValue(Keys.ttsEnabled) ?? "1") == "1"
        if let raw = store.stateValue(Keys.modelTier), let tier = ModelTier(rawValue: raw) {
            modelTier = tier
        } else {
            modelTier = ModelCatalog.autoTier()
        }
    }

    // Pending-turn marker: a notification reply whose inference hasn't run yet
    // (plan §7 fallback — entry is stored, inference happens next foreground).
    var pendingEntryId: Int64? {
        get { store.stateValue(Keys.pendingEntryId).flatMap(Int64.init) }
        set { store.setStateValue(Keys.pendingEntryId, newValue.map(String.init)) }
    }

    var didRequestNotifications: Bool {
        get { store.stateValue(Keys.didRequestNotifications) == "1" }
        set { store.setStateValue(Keys.didRequestNotifications, newValue ? "1" : "0") }
    }
}
