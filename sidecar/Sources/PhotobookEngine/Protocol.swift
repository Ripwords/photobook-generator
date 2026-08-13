import Foundation

enum RequestKind: String, Codable {
    case ping
    case analyze
    case benchmark
}

struct Request: Codable {
    let id: String
    let kind: RequestKind
    var paths: [String]?
    /// Directory to write contact-sheet thumbnails into for `.analyze`
    /// requests, or per-photo thumbnails for `.benchmark` requests when the
    /// thumbnail-write stage should be timed too. Rust owns `app_data_dir`
    /// and passes it in rather than Swift hardcoding a location. Nil (or
    /// omitted) disables thumbnail writing -- used by ping and by
    /// callers/tests that don't care about thumbnails.
    var thumbnailDir: String?
}

struct PongResult: Codable { let version: String }
struct ErrorResult: Codable { let message: String }

enum ResponseResult: Codable {
    case pong(PongResult)
    case error(ErrorResult)
    case analyzed([PhotoRecord])
    case benchmarked([BenchmarkRecord])

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
        case .analyzed(let v):
            try c.encode("analyzed", forKey: .type)
            try c.encode(v, forKey: .data)
        case .benchmarked(let v):
            try c.encode("benchmarked", forKey: .type)
            try c.encode(v, forKey: .data)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "pong": self = .pong(try c.decode(PongResult.self, forKey: .data))
        case "analyzed": self = .analyzed(try c.decode([PhotoRecord].self, forKey: .data))
        case "benchmarked": self = .benchmarked(try c.decode([BenchmarkRecord].self, forKey: .data))
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
