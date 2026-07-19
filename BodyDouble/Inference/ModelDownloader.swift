// First-launch model weight download (plan §2): background URLSession with
// resume support + SHA-256 verification before the file is accepted.

import Foundation
import CryptoKit

final class ModelDownloader: NSObject, ObservableObject {
    enum Phase: Equatable {
        case idle
        case downloading(progress: Double)
        case verifying
        case done
        case failed(String)
    }

    @MainActor @Published private(set) var phase: Phase = .idle
    @MainActor private(set) var activeTier: ModelTier?

    private var task: URLSessionDownloadTask?

    private lazy var session: URLSession = {
        let config = URLSessionConfiguration.background(
            withIdentifier: "com.jacyp.bodydouble.modeldownload")
        config.isDiscretionary = false
        config.sessionSendsLaunchEvents = true
        return URLSession(configuration: config, delegate: self, delegateQueue: nil)
    }()

    private func resumeDataURL(for tier: ModelTier) -> URL {
        ModelCatalog.modelsDirectory()
            .appendingPathComponent("\(tier.rawValue).resumedata")
    }

    @MainActor
    func start(tier: ModelTier) {
        guard !ModelCatalog.isDownloaded(tier) else {
            phase = .done
            return
        }
        activeTier = tier
        phase = .downloading(progress: 0)
        let spec = ModelCatalog.spec(for: tier)
        if let resumeData = try? Data(contentsOf: resumeDataURL(for: tier)) {
            try? FileManager.default.removeItem(at: resumeDataURL(for: tier))
            task = session.downloadTask(withResumeData: resumeData)
        } else {
            task = session.downloadTask(with: spec.downloadURL)
        }
        task?.resume()
    }

    @MainActor
    func cancel() {
        guard let tier = activeTier else { return }
        task?.cancel { [weak self] resumeData in
            guard let self else { return }
            if let resumeData {
                try? resumeData.write(to: self.resumeDataURL(for: tier))
            }
        }
        phase = .idle
    }

    // MARK: - Verification

    private static func sha256OfFile(at url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        while autoreleasepool(invoking: {
            let chunk = handle.readData(ofLength: 4 * 1024 * 1024)
            guard !chunk.isEmpty else { return false }
            hasher.update(data: chunk)
            return true
        }) {}
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }
}

extension ModelDownloader: URLSessionDownloadDelegate {
    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask,
                    didWriteData bytesWritten: Int64, totalBytesWritten: Int64,
                    totalBytesExpectedToWrite: Int64) {
        let expected = totalBytesExpectedToWrite > 0
            ? totalBytesExpectedToWrite
            : ModelCatalog.spec(for: .qwen3_4b).approxSizeBytes
        let progress = Double(totalBytesWritten) / Double(expected)
        Task { @MainActor in
            self.phase = .downloading(progress: min(progress, 1.0))
        }
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask,
                    didFinishDownloadingTo location: URL) {
        // This callback is synchronous — the temp file vanishes when we return.
        // Move it aside immediately, then verify.
        let staging = ModelCatalog.modelsDirectory()
            .appendingPathComponent("incoming-\(UUID().uuidString).gguf")
        do {
            try FileManager.default.moveItem(at: location, to: staging)
        } catch {
            Task { @MainActor in self.phase = .failed("Could not save download: \(error.localizedDescription)") }
            return
        }

        Task { @MainActor in
            guard let tier = self.activeTier else { return }
            self.phase = .verifying
            let spec = ModelCatalog.spec(for: tier)
            let result: Result<Void, Error> = await Task.detached(priority: .userInitiated) {
                if !spec.sha256.isEmpty {
                    let digest = try ModelDownloader.sha256OfFile(at: staging)
                    guard digest == spec.sha256.lowercased() else {
                        throw NSError(domain: "ModelDownloader", code: 1, userInfo: [
                            NSLocalizedDescriptionKey: "Checksum mismatch — download corrupted."])
                    }
                }
                let dest = ModelCatalog.localURL(for: tier)
                try? FileManager.default.removeItem(at: dest)
                try FileManager.default.moveItem(at: staging, to: dest)
                return
            }.result

            switch result {
            case .success:
                self.phase = .done
            case .failure(let error):
                try? FileManager.default.removeItem(at: staging)
                self.phase = .failed(error.localizedDescription)
            }
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask,
                    didCompleteWithError error: Error?) {
        guard let error = error as NSError? else { return }
        // User-initiated cancel already handled resume data in cancel().
        if error.code == NSURLErrorCancelled { return }
        Task { @MainActor in
            if let tier = self.activeTier,
               let resumeData = error.userInfo[NSURLSessionDownloadTaskResumeData] as? Data {
                try? resumeData.write(to: self.resumeDataURL(for: tier))
            }
            self.phase = .failed(error.localizedDescription)
        }
    }
}
