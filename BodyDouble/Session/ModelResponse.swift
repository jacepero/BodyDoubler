// Decoding of the model's JSON output (plan §5). The grammar guarantees shape,
// but parsing stays deliberately tolerant anyway: unknown tools survive decode
// (ToolExecutor drops them), bad args become empty, and stray text around the
// JSON object is stripped. ToolExecutor must never crash (plan §10).

import Foundation

enum JSONValue: Decodable, Equatable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case null
    case array([JSONValue])
    case object([String: JSONValue])

    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let b = try? c.decode(Bool.self) { self = .bool(b) }
        else if let n = try? c.decode(Double.self) { self = .number(n) }
        else if let s = try? c.decode(String.self) { self = .string(s) }
        else if let a = try? c.decode([JSONValue].self) { self = .array(a) }
        else if let o = try? c.decode([String: JSONValue].self) { self = .object(o) }
        else {
            throw DecodingError.dataCorruptedError(in: c, debugDescription: "Unknown JSON value")
        }
    }

    var stringValue: String? {
        if case .string(let s) = self { return s }
        return nil
    }

    var intValue: Int64? {
        switch self {
        case .number(let d): return Int64(exactly: d.rounded())
        case .string(let s): return Int64(s)
        default: return nil
        }
    }
}

struct RawAction: Decodable, Equatable {
    let tool: String
    let args: [String: JSONValue]

    enum CodingKeys: String, CodingKey { case tool, args }

    init(tool: String, args: [String: JSONValue]) {
        self.tool = tool
        self.args = args
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        tool = (try? c.decode(String.self, forKey: .tool)) ?? ""
        args = (try? c.decode([String: JSONValue].self, forKey: .args)) ?? [:]
    }
}

struct ModelResponse: Decodable, Equatable {
    let speech: String
    let actions: [RawAction]

    enum CodingKeys: String, CodingKey { case speech, actions }

    init(speech: String, actions: [RawAction]) {
        self.speech = speech
        self.actions = actions
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        speech = (try? c.decode(String.self, forKey: .speech)) ?? ""
        actions = (try? c.decode([RawAction].self, forKey: .actions)) ?? []
    }

    enum ParseError: Error { case noJSONObject, undecodable }

    /// Extracts the first balanced top-level `{...}` (safety net for any text
    /// around the object) and decodes it.
    static func parse(_ raw: String) throws -> ModelResponse {
        guard let json = firstJSONObject(in: raw) else { throw ParseError.noJSONObject }
        do {
            return try JSONDecoder().decode(ModelResponse.self, from: Data(json.utf8))
        } catch {
            throw ParseError.undecodable
        }
    }

    static func firstJSONObject(in text: String) -> String? {
        var depth = 0
        var inString = false
        var escaped = false
        var start: String.Index?
        for i in text.indices {
            let ch = text[i]
            if inString {
                if escaped { escaped = false }
                else if ch == "\\" { escaped = true }
                else if ch == "\"" { inString = false }
                continue
            }
            switch ch {
            case "\"": inString = true
            case "{":
                if depth == 0 { start = i }
                depth += 1
            case "}":
                depth -= 1
                if depth == 0, let s = start {
                    return String(text[s...i])
                }
            default: break
            }
        }
        return nil
    }
}
