import CoreLocation
import Foundation

/// The parts of a placemark a chapter can be named from.
struct PlaceParts: Equatable {
    var locality: String?
    var administrativeArea: String?
    var country: String?
}

/// Names chapter centroids for the contact sheet's headers.
///
/// This is the app's one outbound call with location data in it: Apple's
/// reverse geocoder receives the coordinates, never a photo. Rust only sends
/// a `geocode` request when the book's Places option is on.
enum Geocoder {
    /// How long one lookup may take before it counts as a failure.
    static let timeout: TimeInterval = 10

    /// The town, else the region, else the country. Blank parts are skipped.
    static func name(of parts: PlaceParts) -> String? {
        [parts.locality, parts.administrativeArea, parts.country]
            .lazy
            .compactMap { $0?.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first { !$0.isEmpty }
    }

    /// One name or nil per `[lat, lon]`, in order. `lookup` runs once per
    /// valid entry, one after another: CLGeocoder allows one request in
    /// flight and throttles bursts. Any failure is that entry's nil.
    static func names(
        for coordinates: [[Double]],
        lookup: (_ latitude: Double, _ longitude: Double) -> PlaceParts?
    ) -> [String?] {
        coordinates.map { pair in
            guard pair.count == 2,
                  CLLocationCoordinate2DIsValid(CLLocationCoordinate2D(latitude: pair[0], longitude: pair[1]))
            else { return nil }
            return lookup(pair[0], pair[1]).flatMap(name(of:))
        }
    }

    /// Asks Apple, blocking until it answers, fails or times out.
    ///
    /// CLGeocoder delivers its answer on the main queue, and the request
    /// loop runs on the main thread, so waiting on a semaphore there would
    /// deadlock. Spinning the run loop lets the main queue drain meanwhile.
    static func appleLookup(latitude: Double, longitude: Double) -> PlaceParts? {
        let answer = Answer()
        let geocoder = CLGeocoder()
        geocoder.reverseGeocodeLocation(CLLocation(latitude: latitude, longitude: longitude)) { placemarks, error in
            if let error {
                FileHandle.standardError.write(Data("PhotobookEngine: geocode failed: \(error)\n".utf8))
            }
            answer.set(placemarks?.first.map {
                PlaceParts(locality: $0.locality, administrativeArea: $0.administrativeArea, country: $0.country)
            })
        }
        let deadline = Date(timeIntervalSinceNow: timeout)
        while !answer.done, Date() < deadline {
            RunLoop.current.run(mode: .default, before: min(deadline, Date(timeIntervalSinceNow: 0.05)))
        }
        if !answer.done { geocoder.cancelGeocode() }
        return answer.parts
    }
}

private final class Answer: @unchecked Sendable {
    private let lock = NSLock()
    private var finished = false
    private var value: PlaceParts?

    var done: Bool { lock.withLock { finished } }
    var parts: PlaceParts? { lock.withLock { value } }

    func set(_ parts: PlaceParts?) {
        lock.withLock {
            value = parts
            finished = true
        }
    }
}
