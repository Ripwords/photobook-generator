import Foundation

enum RequestKind: String, Codable {
    case ping
    case analyze
    case benchmark
    case calibrate
    case export
    case geocode
}

/// One photo to crop and write, as decided by the Rust layout engine.
///
/// `cropX/Y/W/H` are normalised to 0...1 of the photo's own ORIENTED
/// (displayed) frame, top-left origin -- the same space
/// `book::crop::choose_crop` produces on the Rust side, so nothing converts
/// coordinates on the wire. They are resolved into source pixels at export,
/// after EXIF orientation has been applied.
///
/// `filename` is the output basename WITHOUT an extension; the exporter
/// appends the one that matches the format it chose from the source.
struct ExportItem: Codable {
    let sourcePath: String
    let filename: String
    let cropX: Double
    let cropY: Double
    let cropW: Double
    let cropH: Double
}

struct ExportRequest: Codable {
    let outputDir: String
    let items: [ExportItem]
}

/// One record per `ExportItem`, in input order.
///
/// Encoded as a flat tagged union with a `type` discriminator, matching
/// `ResponseResult` below. Rust pins this shape from its own side, so the arms
/// are hand-written rather than synthesised: a mismatch here degrades into
/// "every photo failed" with no cause reported anywhere.
enum ExportRecord: Codable {
    case ok(path: String, width: Int, height: Int, bytes: Int)
    case failed(filename: String, message: String)

    private enum CodingKeys: String, CodingKey {
        case type, path, width, height, bytes, filename, message
    }

    func encode(to encoder: Encoder) throws {
        var c = encoder.container(keyedBy: CodingKeys.self)
        switch self {
        case .ok(let path, let width, let height, let bytes):
            try c.encode("ok", forKey: .type)
            try c.encode(path, forKey: .path)
            try c.encode(width, forKey: .width)
            try c.encode(height, forKey: .height)
            try c.encode(bytes, forKey: .bytes)
        case .failed(let filename, let message):
            try c.encode("failed", forKey: .type)
            try c.encode(filename, forKey: .filename)
            try c.encode(message, forKey: .message)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "ok":
            self = .ok(
                path: try c.decode(String.self, forKey: .path),
                width: try c.decode(Int.self, forKey: .width),
                height: try c.decode(Int.self, forKey: .height),
                bytes: try c.decode(Int.self, forKey: .bytes)
            )
        default:
            self = .failed(
                filename: try c.decode(String.self, forKey: .filename),
                message: try c.decode(String.self, forKey: .message)
            )
        }
    }
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
    /// Payload for `.export` requests. Nil for every other kind; an
    /// `.export` request without it is answered with an error rather than an
    /// empty result, so a caller that forgets it hears about it.
    var export: ExportRequest?
    /// `[lat, lon]` pairs for `.geocode` requests. Nil for every other kind.
    var coordinates: [[Double]]?
}

struct PongResult: Codable { let version: String }
struct ErrorResult: Codable { let message: String }

enum ResponseResult: Codable {
    case pong(PongResult)
    case error(ErrorResult)
    case analyzed([PhotoRecord])
    case benchmarked([BenchmarkRecord])
    case calibrated([CalibrationRecord])
    case exported([ExportRecord])
    /// One place name per requested coordinate, in order; null where the
    /// lookup failed.
    case geocoded([String?])

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
        case .calibrated(let v):
            try c.encode("calibrated", forKey: .type)
            try c.encode(v, forKey: .data)
        case .exported(let v):
            try c.encode("exported", forKey: .type)
            try c.encode(v, forKey: .data)
        case .geocoded(let v):
            try c.encode("geocoded", forKey: .type)
            try c.encode(v, forKey: .data)
        }
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .type) {
        case "pong": self = .pong(try c.decode(PongResult.self, forKey: .data))
        case "analyzed": self = .analyzed(try c.decode([PhotoRecord].self, forKey: .data))
        case "benchmarked": self = .benchmarked(try c.decode([BenchmarkRecord].self, forKey: .data))
        case "calibrated": self = .calibrated(try c.decode([CalibrationRecord].self, forKey: .data))
        case "exported": self = .exported(try c.decode([ExportRecord].self, forKey: .data))
        case "geocoded": self = .geocoded(try c.decode([String?].self, forKey: .data))
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
