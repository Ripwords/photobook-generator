import Foundation
import ImageIO

struct ExifData: Codable {
    /// The camera's wall clock, read AS IF it were UTC. Deliberately not
    /// corrected by `captureOffsetSec`: chapter titles and time-of-day read
    /// it as local time. Rust subtracts the offset only to order photos.
    var captureDate: Date?
    /// Seconds east of UTC from `OffsetTimeOriginal` (else `OffsetTime`).
    /// Phones write it; most dedicated cameras do not, so nil is ordinary.
    var captureOffsetSec: Int?
    var latitude: Double?
    var longitude: Double?
    var pixelWidth: Int
    var pixelHeight: Int
    var make: String?
    var model: String?
    var flashFired: Bool?
}

enum ExifReader {
    enum ReadError: Error { case unreadable(String) }

    private static let formatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.calendar = Calendar(identifier: .gregorian)
        f.dateFormat = "yyyy:MM:dd HH:mm:ss"
        f.timeZone = TimeZone(secondsFromGMT: 0)
        return f
    }()

    /// Parses an EXIF offset tag ("+09:00", "-05:30") into seconds east of
    /// UTC. Anything else -- including the blank "   :  " some cameras
    /// write -- is nil rather than a guess.
    static func offsetSeconds(_ raw: String) -> Int? {
        let chars = Array(raw)
        guard chars.count == 6, chars[3] == ":",
              let sign = chars.first.flatMap({ $0 == "+" ? 1 : $0 == "-" ? -1 : nil }),
              let hours = Int(String(chars[1...2])), let minutes = Int(String(chars[4...5])),
              chars[1...2].allSatisfy(\.isNumber), chars[4...5].allSatisfy(\.isNumber),
              hours <= 14, minutes < 60
        else { return nil }
        return sign * (hours * 3600 + minutes * 60)
    }

    /// Orientations 5-8 are the four that swap width and height.
    private static func swapsAxes(_ orientation: Int) -> Bool {
        (5...8).contains(orientation)
    }

    static func read(path: String) throws -> ExifData {
        let url = URL(fileURLWithPath: path) as CFURL
        guard let source = CGImageSourceCreateWithURL(url, nil),
              let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
        else { throw ReadError.unreadable(path) }

        let rawW = props[kCGImagePropertyPixelWidth] as? Int ?? 0
        let rawH = props[kCGImagePropertyPixelHeight] as? Int ?? 0
        let orientation = props[kCGImagePropertyOrientation] as? Int ?? 1
        let (w, h) = swapsAxes(orientation) ? (rawH, rawW) : (rawW, rawH)

        let exif = props[kCGImagePropertyExifDictionary] as? [CFString: Any]
        let tiff = props[kCGImagePropertyTIFFDictionary] as? [CFString: Any]
        let gps = props[kCGImagePropertyGPSDictionary] as? [CFString: Any]

        var lat = gps?[kCGImagePropertyGPSLatitude] as? Double
        if let ref = gps?[kCGImagePropertyGPSLatitudeRef] as? String, ref == "S", let v = lat {
            lat = -v
        }
        var lon = gps?[kCGImagePropertyGPSLongitude] as? Double
        if let ref = gps?[kCGImagePropertyGPSLongitudeRef] as? String, ref == "W", let v = lon {
            lon = -v
        }

        var flash: Bool?
        if let raw = exif?[kCGImagePropertyExifFlash] as? Int { flash = (raw & 1) == 1 }

        return ExifData(
            captureDate: (exif?[kCGImagePropertyExifDateTimeOriginal] as? String).flatMap(formatter.date(from:)),
            captureOffsetSec: ((exif?[kCGImagePropertyExifOffsetTimeOriginal] as? String).flatMap(offsetSeconds))
                ?? ((exif?[kCGImagePropertyExifOffsetTime] as? String).flatMap(offsetSeconds)),
            latitude: lat,
            longitude: lon,
            pixelWidth: w,
            pixelHeight: h,
            make: tiff?[kCGImagePropertyTIFFMake] as? String,
            model: tiff?[kCGImagePropertyTIFFModel] as? String,
            flashFired: flash
        )
    }
}
