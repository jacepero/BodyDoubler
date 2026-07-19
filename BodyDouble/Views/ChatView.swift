// The main entry view (plan §3): today's turns, a composer, push-to-talk,
// and a model-status banner when the weights aren't downloaded yet.

import SwiftUI

struct ChatView: View {
    @EnvironmentObject private var coordinator: SessionCoordinator
    @EnvironmentObject private var settings: AppSettings
    @EnvironmentObject private var downloader: ModelDownloader
    @EnvironmentObject private var speech: SpeechIO

    @State private var draft = ""
    @FocusState private var composerFocused: Bool

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                modelBanner
                turnsList
                composer
            }
            .navigationTitle("Body Double")
            .navigationBarTitleDisplayMode(.inline)
        }
    }

    // MARK: - Model status banner

    @ViewBuilder
    private var modelBanner: some View {
        if !ModelCatalog.isDownloaded(settings.modelTier) {
            VStack(spacing: 6) {
                switch downloader.phase {
                case .downloading(let progress):
                    ProgressView(value: progress) {
                        Text("Downloading model… \(Int(progress * 100))%")
                            .font(.footnote)
                    }
                case .verifying:
                    Label("Verifying download…", systemImage: "checkmark.shield")
                        .font(.footnote)
                case .failed(let message):
                    Label(message, systemImage: "exclamationmark.triangle")
                        .font(.footnote)
                        .foregroundStyle(.red)
                    downloadButton
                default:
                    Label("The AI model isn't downloaded yet — entries still save.",
                          systemImage: "arrow.down.circle")
                        .font(.footnote)
                    downloadButton
                }
            }
            .padding(10)
            .frame(maxWidth: .infinity)
            .background(.yellow.opacity(0.15))
        }
    }

    private var downloadButton: some View {
        Button("Download \(ModelCatalog.spec(for: settings.modelTier).displayName)") {
            downloader.start(tier: settings.modelTier)
        }
        .font(.footnote.bold())
    }

    // MARK: - Turns

    private var turnsList: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if coordinator.turns.isEmpty {
                        ContentUnavailableView(
                            "Narrate your day",
                            systemImage: "quote.opening",
                            description: Text("Say or type what you're doing. Everything stays on this device."))
                            .padding(.top, 60)
                    }
                    ForEach(coordinator.turns) { turn in
                        TurnBubble(turn: turn)
                            .id(turn.id)
                    }
                }
                .padding()
            }
            .onChange(of: coordinator.turns.count) { _, _ in
                if let last = coordinator.turns.last {
                    withAnimation { proxy.scrollTo(last.id, anchor: .bottom) }
                }
            }
        }
    }

    // MARK: - Composer

    private var composer: some View {
        HStack(spacing: 10) {
            micButton
            TextField("What are you up to?", text: $draft, axis: .vertical)
                .lineLimit(1...4)
                .textFieldStyle(.roundedBorder)
                .focused($composerFocused)
                .onSubmit(send)
            Button(action: send) {
                Image(systemName: "arrow.up.circle.fill")
                    .font(.title2)
            }
            .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                      || coordinator.isGenerating)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(.bar)
    }

    private var micButton: some View {
        Button {
            if speech.isRecording {
                speech.stopRecording()
                let text = speech.transcript
                speech.transcript = ""
                if !text.isEmpty {
                    Task { await coordinator.submit(text, source: .voice) }
                }
            } else {
                Task {
                    guard await speech.requestAuthorization() else { return }
                    try? speech.startRecording()
                }
            }
        } label: {
            Image(systemName: speech.isRecording ? "mic.fill" : "mic")
                .font(.title2)
                .foregroundStyle(speech.isRecording ? .red : .accentColor)
                .symbolEffect(.pulse, isActive: speech.isRecording)
        }
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        Task { await coordinator.submit(text, source: .text) }
    }
}

// MARK: - Bubble

private struct TurnBubble: View {
    let turn: SessionCoordinator.Turn

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(timeString)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                if turn.entry.source == EntrySource.notificationReply.rawValue {
                    Image(systemName: "bell")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                } else if turn.entry.source == EntrySource.voice.rawValue {
                    Image(systemName: "mic")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                if let mood = turn.entry.mood {
                    Text(mood)
                        .font(.caption2)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(Capsule().fill(.blue.opacity(0.15)))
                }
            }
            Text(turn.entry.rawText)
                .padding(10)
                .background(RoundedRectangle(cornerRadius: 12).fill(.blue.opacity(0.12)))

            if turn.inFlight {
                ProgressView().controlSize(.small)
            } else if let speech = turn.speech, !speech.isEmpty {
                Text(speech)
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 12).fill(.gray.opacity(0.15)))
            } else if turn.failed {
                Label("Saved — reply will come when the model is ready.",
                      systemImage: "checkmark.circle")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private var timeString: String {
        turn.entry.createdDate.map(SessionCoordinator.timeOnly) ?? ""
    }
}
