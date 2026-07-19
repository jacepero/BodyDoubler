// Timeline of all journal entries grouped by day (plan §3), with the Phase 4
// on-demand daily recap. (Named JournalTimelineView to avoid SwiftUI.TimelineView.)

import SwiftUI

struct JournalTimelineView: View {
    @EnvironmentObject private var coordinator: SessionCoordinator

    @State private var entries: [Entry] = []
    @State private var summaryDay: SummaryDay?

    private struct SummaryDay: Identifiable {
        let date: Date
        var id: TimeInterval { date.timeIntervalSince1970 }
    }

    private var groupedByDay: [(day: String, items: [Entry])] {
        let groups = Dictionary(grouping: entries) { entry in
            entry.createdDate.map(SessionCoordinator.dayStamp) ?? "unknown"
        }
        return groups.sorted { $0.key > $1.key }
            .map { (day: $0.key, items: $0.value.sorted { ($0.id ?? 0) > ($1.id ?? 0) }) }
    }

    var body: some View {
        NavigationStack {
            List {
                if entries.isEmpty {
                    ContentUnavailableView("No entries yet", systemImage: "clock")
                }
                ForEach(groupedByDay, id: \.day) { group in
                    Section(group.day) {
                        ForEach(group.items) { entry in
                            EntryRow(entry: entry)
                        }
                    }
                }
            }
            .navigationTitle("Timeline")
            .toolbar {
                Button {
                    summaryDay = SummaryDay(date: Date())
                } label: {
                    Label("Daily recap", systemImage: "text.badge.star")
                }
            }
            .onAppear(perform: reload)
            .onChange(of: coordinator.turns) { _, _ in reload() }
            .sheet(item: $summaryDay) { day in
                DailySummaryView(day: day.date)
            }
        }
    }

    private func reload() {
        entries = (try? coordinator.store.allEntries()) ?? []
    }
}

private struct EntryRow: View {
    let entry: Entry

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(entry.createdDate.map(SessionCoordinator.timeOnly) ?? "")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
                if let mood = entry.mood {
                    Text(mood)
                        .font(.caption2)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(Capsule().fill(.blue.opacity(0.15)))
                }
                Spacer()
                Text(entry.source.replacingOccurrences(of: "_", with: " "))
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            Text(entry.rawText)
            if let summary = entry.summary {
                Text(summary)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .italic()
            }
        }
        .padding(.vertical, 2)
    }
}
