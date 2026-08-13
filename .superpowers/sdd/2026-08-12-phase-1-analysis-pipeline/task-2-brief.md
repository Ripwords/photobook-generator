### Task 2: Swift package and NDJSON protocol loop

**Files:**
- Create: `sidecar/Package.swift`, `sidecar/Sources/PhotobookEngine/Protocol.swift`, `sidecar/Sources/PhotobookEngine/main.swift`
- Test: `sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift`

**Interfaces:**
- Consumes: nothing
- Produces: `Request` / `Response` Codable types; a binary that reads one JSON object per line from stdin and writes one per line to stdout. Request kinds in this task: `ping`. Every response carries the request's `id`.

- [ ] **Step 1: Write the failing test**

`sidecar/Tests/PhotobookEngineTests/ProtocolTests.swift`:

```swift
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `swift test --package-path sidecar`
Expected: FAIL — no such package / cannot find `Request` in scope

- [ ] **Step 3: Write minimal implementation**

`sidecar/Package.swift`:

```swift
// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "PhotobookEngine",
    platforms: [.macOS(.v15)],
    targets: [
        .executableTarget(name: "PhotobookEngine"),
        .testTarget(name: "PhotobookEngineTests", dependencies: ["PhotobookEngine"]),
    ]
)
```

`sidecar/Sources/PhotobookEngine/Protocol.swift`:

```swift
import Foundation

enum RequestKind: String, Codable {
    case ping
    case analyze
}

struct Request: Codable {
    let id: String
    let kind: RequestKind
    var paths: [String]?
}

struct PongResult: Codable { let version: String }
struct ErrorResult: Codable { let message: String }

enum ResponseResult: Codable {
    case pong(PongResult)
    case error(ErrorResult)

    private enum CodingKeys: String, CodingKey { case type, data }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .pong(let v):
            try c.encode("pong", forKey: .type)
            try c.encode(v, forKey: .data)
        case .error(let v):
            try c.encode("error", forKey: .type)
            try c.encode(v, forKey: .data)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "pong": self = .pong(try c.decode(PongResult.self, forKey: .data))
        default: self = .error(try c.decode(ErrorResult.self, forKey: .data))
        }
    }
}

struct Response: Codable {
    let id: String
    let result: ResponseResult
}

enum Engine {
    static let version = "0.1.0"
}
```

`sidecar/Sources/PhotobookEngine/main.swift`:

```swift
import Foundation

let encoder = JSONEncoder()
// Swift's default `.deferredToDate` encodes seconds since 2001-01-01, which
// Rust would silently misread as a Unix epoch. Be explicit.
encoder.dateEncodingStrategy = .secondsSince1970

let decoder = JSONDecoder()
decoder.dateDecodingStrategy = .secondsSince1970

func emit(_ response: Response) {
    guard let data = try? encoder.encode(response) else { return }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data("\n".utf8))
}

while let line = readLine(strippingNewline: true) {
    if line.isEmpty { continue }
    guard let request = try? decoder.decode(Request.self, from: Data(line.utf8)) else {
        emit(Response(id: "unknown", result: .error(ErrorResult(message: "malformed request"))))
        continue
    }
    switch request.kind {
    case .ping:
        emit(Response(id: request.id, result: .pong(PongResult(version: Engine.version))))
    case .analyze:
        emit(Response(id: request.id, result: .error(ErrorResult(message: "not implemented"))))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `swift test --package-path sidecar`
Expected: PASS, 3 tests

Then verify the loop by hand:

```bash
echo '{"id":"a","kind":"ping"}' | swift run --package-path sidecar PhotobookEngine
```

Expected: `{"id":"a","result":{"type":"pong","data":{"version":"0.1.0"}}}`

- [ ] **Step 5: Commit**

```bash
git add sidecar
git commit -m "feat(sidecar): add NDJSON protocol loop with ping"
```

---

