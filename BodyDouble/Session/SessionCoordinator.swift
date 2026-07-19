// The heart of the app (plan §3): assembles the per-turn context block,
// runs inference, hands actions to ToolExecutor, and speaks the reply.
//
// Invariant (plan §10): the raw entry is inserted BEFORE inference is
// attempted, so a user utterance is stored 100% of the time even when the
// model fails, isn't downloaded, or the background window runs out.

import Foundation
import UIKit

@MainActor
final class SessionCoordinator: ObservableObject {

    struct Turn: Identifiable, Equatable {
        let id: Int64                 // entry id
        var entry: Entry
        var speech: String? = nil
        var failed = false
        var inFlight = false
    }

    @Published private(set) var turns: [Turn] = []
    @Published private(set) var isGenerating = false
    @Published private(set) var lastError: String?

    let store: Store
    let settings: AppSettings
    let engine: InferenceEngine
    let speech: SpeechIO
    /// Set by AppEnvironment after the scheduler exists (benign cycle).
    var scheduler: NotificationScheduler?
    var liveActivity: LiveActivityManager?

    private var executor: ToolExecutor {
        ToolExecutor(store: store) { [weak self] minutes in
            Task { @MainActor in self?.scheduler?.scheduleOneOff(minutesFromNow: minutes) }
        }
    }

    init(store: Store, settings: AppSettings, engine: InferenceEngine, speech: SpeechIO) {
        self.store = store
        self.settings = settings
        self.engine = engine
        self.speech = speech
        loadTodaysTurns()
    }

    private func loadTodaysTurns() {
        let entries = (try? store.entries(on: Date())) ?? []
        turns = entries.compactMap { e in
            guard let id = e.id else { return nil }
            return Turn(id: id, entry: e)
        }
    }

    // MARK: - Foreground turn (typed or push-to-talk)

    func submit(_ text: String, source: EntrySource) async {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        let currentTaskId = ((try? store.currentTask()) ?? nil)?.id
        guard let entry = try? store.insertEntry(rawText: trimmed, source: source,
                                                 taskId: currentTaskId),
              let entryId = entry.id else {
            lastError = "Could not save entry."
            return
        }
        turns.append(Turn(id: entryId, entry: entry, inFlight: true))
        await runTurn(for: entry, speakAllowed: true)
    }

    // MARK: - Notification reply (plan §7)

    /// Called from the notification delegate. Insert is synchronous and
    /// unconditional; inference is best-effort inside a background task window,
    /// falling back to next-foreground processing.
    nonisolated func handleNotificationReply(_ text: String) {
        Task { @MainActor in
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { return }

            let currentTaskId = ((try? self.store.currentTask()) ?? nil)?.id
            guard let entry = try? self.store.insertEntry(
                rawText: trimmed, source: .notificationReply, taskId: currentTaskId),
                  let entryId = entry.id else { return }
            try? self.store.markLatestCheckinResponded()
            self.settings.pendingEntryId = entryId
            self.turns.append(Turn(id: entryId, entry: entry))
            self.scheduler?.rescheduleChain()

            // Best-effort inference in the short background window.
            let bgTask = UIApplication.shared.beginBackgroundTask(withName: "checkin-inference")
            await self.processPendingTurn()
            if bgTask != .invalid {
                UIApplication.shared.endBackgroundTask(bgTask)
            }
        }
    }

    /// Runs the pending notification-reply turn if one exists. Called after a
    /// reply and again on every foregrounding (plan §7 fallback).
    func processPendingTurn() async {
        guard let pendingId = settings.pendingEntryId,
              let entry = try? store.entry(id: pendingId) else { return }
        let speakAllowed = UIApplication.shared.applicationState == .active
        let succeeded = await runTurn(for: entry, speakAllowed: speakAllowed)
        if succeeded {
            settings.pendingEntryId = nil
        }
    }

    // MARK: - Turn pipeline

    @discardableResult
    private func runTurn(for entry: Entry, speakAllowed: Bool) async -> Bool {
        guard let entryId = entry.id else { return false }
        isGenerating = true
        setTurn(entryId) { $0.inFlight = true; $0.failed = false }
        defer {
            isGenerating = false
            setTurn(entryId) { $0.inFlight = false }
        }

        let context = buildContext(userText: entry.rawText, excluding: entryId)
        let tier = settings.modelTier
        let prompt = Prompt.chatML(system: Prompt.system, user: context,
                                   noThink: tier == .qwen3_1_7b)
        do {
            try await engine.ensureLoaded(tier: tier)
            let raw = try await engine.generate(prompt: prompt,
                                                grammarGBNF: ToolGrammar.gbnf)
            let response = try ModelResponse.parse(raw)
            executor.apply(response.actions, entryId: entryId)

            if let refreshed = try? store.entry(id: entryId) {
                setTurn(entryId) { $0.entry = refreshed }
            }
            setTurn(entryId) { $0.speech = response.speech }
            lastError = nil

            if speakAllowed, settings.ttsEnabled, !response.speech.isEmpty {
                speech.speak(response.speech, voiceIdentifier: settings.voiceIdentifier)
            }
            liveActivity?.refresh(from: store)
            return true
        } catch {
            setTurn(entryId) { $0.failed = true }
            lastError = error.localizedDescription
            return false
        }
    }

    private func setTurn(_ id: Int64, _ mutate: (inout Turn) -> Void) {
        guard let idx = turns.firstIndex(where: { $0.id == id }) else { return }
        mutate(&turns[idx])
    }

    // MARK: - Context assembly (plan §6)

    func buildContext(userText: String, excluding entryId: Int64? = nil,
                      now: Date = Date()) -> String {
        var lines: [String] = []
        let recents = (try? store.recentEntries(limit: 5, excluding: entryId)) ?? []

        // [time]
        var timeLine = "[time] \(Self.shortStamp(now))"
        if let last = recents.first?.createdDate {
            let mins = max(0, Int(now.timeIntervalSince(last) / 60))
            timeLine += " (\(mins) min since last entry)"
        }
        lines.append(timeLine)

        // [current task]
        if let task = (try? store.currentTask()) ?? nil {
            let elapsed = task.updatedDate.map { Self.elapsedString(since: $0, now: now) } ?? "?"
            lines.append("[current task] \(task.title) (id \(task.id ?? 0), \(task.status), \(elapsed))")
        } else {
            lines.append("[current task] none")
        }

        // [open threads]
        let threads = (try? store.openThreads()) ?? []
        if threads.isEmpty {
            lines.append("[open threads] none")
        } else {
            let list = threads.map { "(id \($0.id ?? 0)) \($0.note)" }.joined(separator: "; ")
            lines.append("[open threads] \(list)")
        }

        // [recent] — oldest first, capped for the ~1,500 token budget
        if !recents.isEmpty {
            lines.append("[recent]")
            for e in recents.reversed() {
                let time = e.createdDate.map(Self.timeOnly) ?? "?"
                let text = (e.summary ?? e.rawText).prefix(100)
                lines.append("- \(time) \(text)")
            }
        }

        lines.append("[user says] \"\(userText.prefix(500))\"")
        return lines.joined(separator: "\n")
    }

    // MARK: - Daily summary (plan §8 Phase 4)

    func generateDailySummary(for day: Date) async throws -> String {
        let entries = try store.entries(on: day)
        guard !entries.isEmpty else { return "No entries for this day." }

        var lines: [String] = ["Journal entries for \(Self.dayStamp(day)):"]
        for e in entries.prefix(60) {
            let time = e.createdDate.map(Self.timeOnly) ?? "?"
            lines.append("- \(time) \(e.summary ?? String(e.rawText.prefix(120)))")
        }
        let tier = settings.modelTier
        try await engine.ensureLoaded(tier: tier)
        let prompt = Prompt.chatML(system: Prompt.dailySummarySystem,
                                   user: lines.joined(separator: "\n"),
                                   noThink: tier == .qwen3_1_7b)
        return try await engine.generate(prompt: prompt, grammarGBNF: nil, maxTokens: 400)
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    // MARK: - Formatting helpers

    private static let shortStampFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd'T'HH:mm"
        return f
    }()

    private static let timeOnlyFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "HH:mm"
        return f
    }()

    private static let dayStampFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy-MM-dd"
        return f
    }()

    static func shortStamp(_ d: Date) -> String { shortStampFormatter.string(from: d) }
    static func timeOnly(_ d: Date) -> String { timeOnlyFormatter.string(from: d) }
    static func dayStamp(_ d: Date) -> String { dayStampFormatter.string(from: d) }

    static func elapsedString(since start: Date, now: Date = Date()) -> String {
        let mins = max(0, Int(now.timeIntervalSince(start) / 60))
        if mins < 60 { return "\(mins)m" }
        return "\(mins / 60)h \(mins % 60)m"
    }
}
