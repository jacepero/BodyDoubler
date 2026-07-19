// Model tiers, download sources, and device-fit checks (plan §2, §9).
// Weights are downloaded on first launch — never bundled (App Store size).

import Foundation

enum ModelTier: String, Codable, CaseIterable, Identifiable {
    case qwen3_4b = "qwen3-4b"
    case qwen3_1_7b = "qwen3-1.7b"

    var id: String { rawValue }
}

struct ModelSpec {
    let tier: ModelTier
    let displayName: String
    let fileName: String
    let downloadURL: URL
    /// Pin before shipping (README §Model checksums). Empty = verification skipped.
    let sha256: String
    let approxSizeBytes: Int64
    /// Free-RAM headroom required to load (plan §9: 4B Q4 needs ~3 GB).
    let requiredHeadroomBytes: Int64
}

enum ModelCatalog {
    static let specs: [ModelTier: ModelSpec] = [
        .qwen3_4b: ModelSpec(
            tier: .qwen3_4b,
            displayName: "Qwen3 4B Instruct (Q4_K_M, ~2.5 GB)",
            fileName: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
            downloadURL: URL(string: "https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf")!,
            sha256: "",
            approxSizeBytes: 2_600_000_000,
            requiredHeadroomBytes: 3_000_000_000),
        .qwen3_1_7b: ModelSpec(
            tier: .qwen3_1_7b,
            displayName: "Qwen3 1.7B (Q4_K_M, ~1.1 GB)",
            fileName: "Qwen3-1.7B-Q4_K_M.gguf",
            downloadURL: URL(string: "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf")!,
            sha256: "",
            approxSizeBytes: 1_200_000_000,
            requiredHeadroomBytes: 1_600_000_000),
    ]

    static func spec(for tier: ModelTier) -> ModelSpec { specs[tier]! }

    /// Plan §8 Phase 2: devices with < 6 GB RAM get the 1.7B tier.
    static func autoTier() -> ModelTier {
        ProcessInfo.processInfo.physicalMemory < 6_000_000_000 ? .qwen3_1_7b : .qwen3_4b
    }

    /// Runtime headroom check before loading (plan §9: os_proc_available_memory).
    static func canLoadNow(_ tier: ModelTier) -> Bool {
        let available = os_proc_available_memory()
        // 0 means "unavailable" on some contexts (e.g. simulator) — don't block.
        guard available > 0 else { return true }
        return Int64(available) >= spec(for: tier).requiredHeadroomBytes
    }

    static func modelsDirectory() -> URL {
        let dir = (try? FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask,
            appropriateFor: nil, create: true)) ?? FileManager.default.temporaryDirectory
        let models = dir.appendingPathComponent("Models", isDirectory: true)
        try? FileManager.default.createDirectory(at: models, withIntermediateDirectories: true)
        return models
    }

    static func localURL(for tier: ModelTier) -> URL {
        modelsDirectory().appendingPathComponent(spec(for: tier).fileName)
    }

    static func isDownloaded(_ tier: ModelTier) -> Bool {
        FileManager.default.fileExists(atPath: localURL(for: tier).path)
    }

    static func deleteLocal(_ tier: ModelTier) {
        try? FileManager.default.removeItem(at: localURL(for: tier))
    }
}
