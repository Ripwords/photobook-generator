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
