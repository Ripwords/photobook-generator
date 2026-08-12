import Testing
import Foundation
@testable import PhotobookEngine

@Test func decodesPingRequest() throws {
    let json = #"{"id":"abc","kind":"ping"}"#
    let req = try JSONDecoder().decode(Request.self, from: Data(json.utf8))
    #expect(req.id == "abc")
    #expect(req.kind == .ping)
}

@Test func encodesPongResponseWithMatchingId() throws {
    let res = Response(id: "abc", result: .pong(PongResult(version: "0.1.0")))
    let data = try JSONEncoder().encode(res)
    let text = String(decoding: data, as: UTF8.self)
    #expect(text.contains("\"id\":\"abc\""))
    #expect(text.contains("\"version\":\"0.1.0\""))
}

@Test func encodesErrorResponse() throws {
    let res = Response(id: "z", result: .error(ErrorResult(message: "boom")))
    let data = try JSONEncoder().encode(res)
    #expect(String(decoding: data, as: UTF8.self).contains("boom"))
}

@Test func unknownKindEchoesRequestId() throws {
    let line = #"{"id":"real-id","kind":"bogus"}"#
    let response = handle(line: line)
    #expect(response.id == "real-id")
    guard case .error(let err) = response.result else {
        Issue.record("expected an error result")
        return
    }
    #expect(err.message == "malformed request")
}

@Test func invalidJsonProducesUnknownId() throws {
    let response = handle(line: "not json at all")
    #expect(response.id == "unknown")
    guard case .error(let err) = response.result else {
        Issue.record("expected an error result")
        return
    }
    #expect(err.message == "malformed request")
}

@Test func envelopeDecodesGarbageKind() throws {
    let json = #"{"id":"e1","kind":"totally-not-a-kind"}"#
    let envelope = try JSONDecoder().decode(RequestEnvelope.self, from: Data(json.utf8))
    #expect(envelope.id == "e1")
}
