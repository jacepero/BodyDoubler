// Settings (plan §3): check-in interval, quiet hours, voice, model tier +
// download management, and export (plan §8 Phase 4).

import SwiftUI
import AVFoundation

struct SettingsView: View {
    @EnvironmentObject private var env: AppEnvironment
    @EnvironmentObject private var settings: AppSettings
    @EnvironmentObject private var downloader: ModelDownloader
    @EnvironmentObject private var scheduler: NotificationScheduler

    @State private var exportURL: ExportFile?
    @State private var exportError: String?

    private struct ExportFile: Identifiable {
        let url: URL
        var id: String { url.path }
    }

    var body: some View {
        NavigationStack {
            Form {
                checkinSection
                voiceSection
                modelSection
                dataSection
                aboutSection
            }
            .navigationTitle("Settings")
            .sheet(item: $exportURL) { file in
                ActivityView(items: [file.url])
            }
        }
    }

    // MARK: - Check-ins

    private var checkinSection: some View {
        Section("Check-ins") {
            Stepper(value: $settings.checkinIntervalMinutes, in: 15...120, step: 15) {
                Text("Every \(settings.checkinIntervalMinutes) min")
            }
            .onChange(of: settings.checkinIntervalMinutes) { _, _ in
                scheduler.rescheduleChain()
            }
            Toggle("Quiet hours", isOn: $settings.quietHoursEnabled)
            if settings.quietHoursEnabled {
                Picker("From", selection: $settings.quietStartHour) {
                    ForEach(0..<24, id: \.self) { Text(hourLabel($0)).tag($0) }
                }
                Picker("Until", selection: $settings.quietEndHour) {
                    ForEach(0..<24, id: \.self) { Text(hourLabel($0)).tag($0) }
                }
            }
            if !scheduler.authorized {
                Label("Notifications are off — enable them in the Settings app for check-ins.",
                      systemImage: "bell.slash")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func hourLabel(_ hour: Int) -> String {
        String(format: "%02d:00", hour)
    }

    // MARK: - Voice

    private var voiceSection: some View {
        Section("Voice") {
            Toggle("Speak replies", isOn: $settings.ttsEnabled)
            if settings.ttsEnabled {
                Picker("Voice", selection: Binding(
                    get: { settings.voiceIdentifier ?? "" },
                    set: { settings.voiceIdentifier = $0.isEmpty ? nil : $0 })) {
                    Text("Automatic (best installed)").tag("")
                    ForEach(availableVoices, id: \.identifier) { voice in
                        Text("\(voice.name) (\(voice.language))\(qualityTag(voice))")
                            .tag(voice.identifier)
                    }
                }
            }
        }
    }

    private var availableVoices: [AVSpeechSynthesisVoice] {
        let langPrefix = Locale.current.language.languageCode?.identifier ?? "en"
        return AVSpeechSynthesisVoice.speechVoices()
            .filter { $0.language.hasPrefix(langPrefix) }
            .sorted { $0.name < $1.name }
    }

    private func qualityTag(_ voice: AVSpeechSynthesisVoice) -> String {
        switch voice.quality {
        case .premium: return " ★★"
        case .enhanced: return " ★"
        default: return ""
        }
    }

    // MARK: - Model

    private var modelSection: some View {
        Section {
            Picker("Model", selection: $settings.modelTier) {
                ForEach(ModelTier.allCases) { tier in
                    Text(ModelCatalog.spec(for: tier).displayName).tag(tier)
                }
            }
            .onChange(of: settings.modelTier) { _, _ in
                Task { await env.engine.unload() }
            }

            switch downloader.phase {
            case .downloading(let progress):
                ProgressView(value: progress) {
                    Text("Downloading… \(Int(progress * 100))%")
                }
                Button("Cancel download", role: .destructive) { downloader.cancel() }
            case .verifying:
                Label("Verifying checksum…", systemImage: "checkmark.shield")
            default:
                if ModelCatalog.isDownloaded(settings.modelTier) {
                    Label("Downloaded", systemImage: "checkmark.circle.fill")
                        .foregroundStyle(.green)
                    Button("Delete model file", role: .destructive) {
                        Task {
                            await env.engine.unload()
                            ModelCatalog.deleteLocal(settings.modelTier)
                        }
                    }
                } else {
                    Button("Download model") { downloader.start(tier: settings.modelTier) }
                    if case .failed(let message) = downloader.phase {
                        Text(message).font(.footnote).foregroundStyle(.red)
                    }
                }
            }
        } header: {
            Text("Model")
        } footer: {
            Text("Recommended for this device: \(ModelCatalog.spec(for: ModelCatalog.autoTier()).displayName). Inference runs entirely on-device.")
        }
    }

    // MARK: - Data

    private var dataSection: some View {
        Section {
            Button {
                export { try Exporter.exportJSON(store: env.store) }
            } label: {
                Label("Export everything (JSON)", systemImage: "square.and.arrow.up")
            }
            Button {
                export { try Exporter.exportMarkdown(store: env.store) }
            } label: {
                Label("Export journal (Markdown)", systemImage: "doc.text")
            }
            if let exportError {
                Text(exportError).font(.footnote).foregroundStyle(.red)
            }
        } header: {
            Text("Data")
        } footer: {
            Text("All data lives on this device. The only network call the app ever makes is the model download.")
        }
    }

    private func export(_ make: () throws -> URL) {
        do {
            exportURL = ExportFile(url: try make())
            exportError = nil
        } catch {
            exportError = error.localizedDescription
        }
    }

    private var aboutSection: some View {
        Section("About") {
            LabeledContent("Version",
                           value: Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "—")
        }
    }
}

// UIActivityViewController wrapper for share sheets.
private struct ActivityView: UIViewControllerRepresentable {
    let items: [Any]

    func makeUIViewController(context: Context) -> UIActivityViewController {
        UIActivityViewController(activityItems: items, applicationActivities: nil)
    }

    func updateUIViewController(_ vc: UIActivityViewController, context: Context) {}
}
