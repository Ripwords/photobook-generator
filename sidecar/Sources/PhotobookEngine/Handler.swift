import Foundation

// This logic is deliberately kept out of main.swift. Functions defined
// alongside top-level code in an executable target's main.swift are not
// safely callable from `@testable import` — the test binary segfaults
// (signal 11) when invoking them, because main.swift's top-level statements
// double as the module's synthesized entry point. Keeping the line-handling
// logic in an ordinary file makes it both testable and safe.

// Swift's default `.deferredToDate` encodes seconds since 2001-01-01, which
// Rust would silently misread as a Unix epoch. Be explicit. (Only main.swift
// may contain top-level executable statements in an executable target, so
// these are built via immediately-invoked closures rather than post-init
// mutation.)
let encoder: JSONEncoder = {
    let e = JSONEncoder()
    e.dateEncodingStrategy = .secondsSince1970
    return e
}()

let decoder: JSONDecoder = {
    let d = JSONDecoder()
    d.dateDecodingStrategy = .secondsSince1970
    return d
}()

/// Minimal shape used to recover a request's `id` when the full `Request`
/// fails to decode (e.g. an unrecognised `kind`), so error responses can
/// still be correlated by the caller instead of coming back as `"unknown"`.
struct RequestEnvelope: Decodable { let id: String }

/// Writes one Response as a line of JSON to stdout. stdout is the protocol
/// channel — diagnostics never go there, only to stderr. If encoding the
/// response itself fails, a minimal fallback error response carrying the
/// same id is written to stdout instead, so the caller is never left
/// waiting on a line that will never arrive.
func emit(_ response: Response) {
    do {
        let data = try encoder.encode(response)
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data("\n".utf8))
    } catch {
        FileHandle.standardError.write(Data("PhotobookEngine: failed to encode response for id \(response.id): \(error)\n".utf8))
        let fallback = Response(id: response.id, result: .error(ErrorResult(message: "internal encoding error")))
        if let fallbackData = try? encoder.encode(fallback) {
            FileHandle.standardOutput.write(fallbackData)
            FileHandle.standardOutput.write(Data("\n".utf8))
        } else {
            // Last-resort hand-built JSON in case even the minimal fallback
            // response fails to encode.
            let escapedId = response.id
                .replacingOccurrences(of: "\\", with: "\\\\")
                .replacingOccurrences(of: "\"", with: "\\\"")
            let raw = "{\"id\":\"\(escapedId)\",\"result\":{\"type\":\"error\",\"data\":{\"message\":\"internal encoding error\"}}}\n"
            FileHandle.standardOutput.write(Data(raw.utf8))
        }
    }
}

/// Decodes and dispatches a single line of NDJSON input into a Response.
/// Falls back to a lenient id-only decode when the full `Request` fails,
/// so malformed-but-identifiable requests still get a correlated error
/// response instead of `id: "unknown"`.
func handle(line: String) -> Response {
    if let request = try? decoder.decode(Request.self, from: Data(line.utf8)) {
        switch request.kind {
        case .ping:
            return Response(id: request.id, result: .pong(PongResult(version: Engine.version)))
        case .analyze:
            return Response(id: request.id, result: .error(ErrorResult(message: "not implemented")))
        }
    }
    let id = (try? decoder.decode(RequestEnvelope.self, from: Data(line.utf8)))?.id ?? "unknown"
    return Response(id: id, result: .error(ErrorResult(message: "malformed request")))
}
