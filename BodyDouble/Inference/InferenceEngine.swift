// llama.cpp wrapper (plan §2): Metal-accelerated, GBNF-grammar-constrained
// generation. The model lazy-loads on first turn (plan §10: cold start < 3 s).
//
// NOTE ON API DRIFT: symbol names here track the 2026 llama.cpp C API
// (llama_model_load_from_file / llama_init_from_model / llama_memory_clear /
// llama_vocab_is_eog). If the pinned revision predates a rename, the compiler
// error points straight at the one-line fix — see README "llama.cpp API drift".

import Foundation
import llama

actor InferenceEngine {
    enum EngineError: LocalizedError {
        case modelFileMissing
        case insufficientMemory
        case loadFailed
        case contextFailed
        case grammarFailed
        case tokenizationFailed
        case promptTooLong(tokens: Int, limit: Int)
        case decodeFailed

        var errorDescription: String? {
            switch self {
            case .modelFileMissing: return "Model not downloaded yet."
            case .insufficientMemory: return "Not enough free memory to load the model."
            case .loadFailed: return "Failed to load the model."
            case .contextFailed: return "Failed to create an inference context."
            case .grammarFailed: return "Grammar failed to compile."
            case .tokenizationFailed: return "Tokenization failed."
            case .promptTooLong(let t, let l): return "Prompt too long (\(t) tokens, limit \(l))."
            case .decodeFailed: return "Inference failed mid-generation."
            }
        }
    }

    static let contextLength: Int32 = 2048   // plan §6: keep context under ~1,500 tokens

    private var model: OpaquePointer?
    private var ctx: OpaquePointer?
    private var vocab: OpaquePointer?
    private(set) var loadedTier: ModelTier?
    private var backendInitialized = false

    // MARK: - Lifecycle

    func ensureLoaded(tier: ModelTier) throws {
        if loadedTier == tier, model != nil { return }
        unload()

        let path = ModelCatalog.localURL(for: tier).path
        guard FileManager.default.fileExists(atPath: path) else {
            throw EngineError.modelFileMissing
        }
        guard ModelCatalog.canLoadNow(tier) else {
            throw EngineError.insufficientMemory
        }

        if !backendInitialized {
            llama_backend_init()
            backendInitialized = true
        }

        var mparams = llama_model_default_params()
        mparams.n_gpu_layers = 99   // full Metal offload
        guard let m = llama_model_load_from_file(path, mparams) else {
            throw EngineError.loadFailed
        }

        var cparams = llama_context_default_params()
        cparams.n_ctx = UInt32(Self.contextLength)
        let threads = Int32(max(2, ProcessInfo.processInfo.activeProcessorCount - 2))
        cparams.n_threads = threads
        cparams.n_threads_batch = threads
        guard let c = llama_init_from_model(m, cparams) else {
            llama_model_free(m)
            throw EngineError.contextFailed
        }

        model = m
        ctx = c
        vocab = llama_model_get_vocab(m)
        loadedTier = tier
    }

    func unload() {
        if let ctx { llama_free(ctx) }
        if let model { llama_model_free(model) }
        ctx = nil
        model = nil
        vocab = nil
        loadedTier = nil
    }

    // MARK: - Generation

    /// One stateless turn (plan §1): full prompt in, full completion out.
    /// `grammarGBNF` constrains output to the tool-call JSON schema; pass nil
    /// for free-text generation (daily summary).
    func generate(prompt: String, grammarGBNF: String?, maxTokens: Int32 = 320) throws -> String {
        guard let ctx, let vocab else { throw EngineError.loadFailed }

        // Each turn starts fresh — clear the KV cache.
        // (Older llama.cpp: llama_kv_cache_clear(ctx).)
        llama_memory_clear(llama_get_memory(ctx), true)

        var tokens = try tokenize(prompt)
        guard tokens.count < Int(Self.contextLength - maxTokens) else {
            throw EngineError.promptTooLong(tokens: tokens.count,
                                            limit: Int(Self.contextLength - maxTokens))
        }

        // Sampler chain: grammar (hard constraint) -> temperature -> sample.
        let chainParams = llama_sampler_chain_default_params()
        guard let chain = llama_sampler_chain_init(chainParams) else {
            throw EngineError.decodeFailed
        }
        defer { llama_sampler_free(chain) }

        if let grammarGBNF {
            guard let grammar = llama_sampler_init_grammar(vocab, grammarGBNF, "root") else {
                throw EngineError.grammarFailed
            }
            llama_sampler_chain_add(chain, grammar)
        }
        llama_sampler_chain_add(chain, llama_sampler_init_temp(0.6))
        llama_sampler_chain_add(chain, llama_sampler_init_dist(UInt32.random(in: .min ... .max)))

        // Decode the prompt.
        let promptOk = tokens.withUnsafeMutableBufferPointer { buf in
            llama_decode(ctx, llama_batch_get_one(buf.baseAddress, Int32(buf.count))) == 0
        }
        guard promptOk else { throw EngineError.decodeFailed }

        // Sample token-by-token until EOG or budget.
        var output = ""
        for _ in 0..<maxTokens {
            let token = llama_sampler_sample(chain, ctx, -1)
            if llama_vocab_is_eog(vocab, token) { break }
            output += piece(for: token)

            var next = token
            let ok = withUnsafeMutablePointer(to: &next) { p in
                llama_decode(ctx, llama_batch_get_one(p, 1)) == 0
            }
            guard ok else { throw EngineError.decodeFailed }
        }
        return output
    }

    // MARK: - Token helpers

    private func tokenize(_ text: String) throws -> [llama_token] {
        guard let vocab else { throw EngineError.loadFailed }
        let utf8Count = Int32(text.utf8.count)
        var tokens = [llama_token](repeating: 0, count: Int(utf8Count) + 16)
        let n = llama_tokenize(vocab, text, utf8Count, &tokens, Int32(tokens.count),
                               /* add_special */ true, /* parse_special */ true)
        guard n >= 0 else { throw EngineError.tokenizationFailed }
        return Array(tokens.prefix(Int(n)))
    }

    private func piece(for token: llama_token) -> String {
        guard let vocab else { return "" }
        var buf = [CChar](repeating: 0, count: 128)
        let n = llama_token_to_piece(vocab, token, &buf, Int32(buf.count),
                                     /* lstrip */ 0, /* special */ false)
        guard n > 0 else { return "" }
        return String(decoding: buf.prefix(Int(n)).map { UInt8(bitPattern: $0) },
                      as: UTF8.self)
    }
}
