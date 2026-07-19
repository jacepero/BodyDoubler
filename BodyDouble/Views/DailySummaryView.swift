// Phase 4: on-demand, model-generated end-of-day recap (free-text generation,
// no grammar).

import SwiftUI

struct DailySummaryView: View {
    let day: Date

    @EnvironmentObject private var coordinator: SessionCoordinator
    @EnvironmentObject private var settings: AppSettings
    @EnvironmentObject private var speech: SpeechIO
    @Environment(\.dismiss) private var dismiss

    @State private var summary: String?
    @State private var error: String?

    var body: some View {
        NavigationStack {
            Group {
                if let summary {
                    ScrollView {
                        Text(summary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding()
                    }
                } else if let error {
                    ContentUnavailableView(error, systemImage: "exclamationmark.triangle")
                } else {
                    ProgressView("Summarizing the day…")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .navigationTitle(SessionCoordinator.dayStamp(day))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
                if let summary, settings.ttsEnabled {
                    ToolbarItem {
                        Button {
                            speech.speak(summary, voiceIdentifier: settings.voiceIdentifier)
                        } label: {
                            Label("Speak", systemImage: "speaker.wave.2")
                        }
                    }
                }
            }
            .task {
                do {
                    summary = try await coordinator.generateDailySummary(for: day)
                } catch {
                    self.error = error.localizedDescription
                }
            }
        }
    }
}
