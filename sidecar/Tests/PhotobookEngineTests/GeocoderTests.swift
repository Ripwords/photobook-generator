import Testing
import Foundation
@testable import PhotobookEngine

@Test func geocodeNamePrefersLocality() {
    let parts = PlaceParts(locality: "Kyoto", administrativeArea: "Kyoto Prefecture", country: "Japan")
    #expect(Geocoder.name(of: parts) == "Kyoto")
}

@Test func geocodeNameFallsBackToAdministrativeAreaThenCountry() {
    #expect(Geocoder.name(of: PlaceParts(locality: nil, administrativeArea: "Bali", country: "Indonesia")) == "Bali")
    #expect(Geocoder.name(of: PlaceParts(locality: nil, administrativeArea: nil, country: "Iceland")) == "Iceland")
}

/// A placemark in open country can carry an empty or blank locality rather
/// than none. A blank name would title a chapter with nothing.
@Test func geocodeNameSkipsBlankParts() {
    #expect(Geocoder.name(of: PlaceParts(locality: "  ", administrativeArea: "", country: "Iceland")) == "Iceland")
    #expect(Geocoder.name(of: PlaceParts(locality: nil, administrativeArea: nil, country: nil)) == nil)
    #expect(Geocoder.name(of: PlaceParts(locality: " Hội An ", administrativeArea: nil, country: nil)) == "Hội An")
}

/// Entry two fails the way an offline or throttled lookup does. It alone is
/// null; the entries either side are still named, in their own positions.
@Test func geocodeNamesAnswerInOrderAndIsolateAFailure() {
    let places: [Double: PlaceParts] = [
        35.01: PlaceParts(locality: "Kyoto", administrativeArea: nil, country: nil),
        64.15: PlaceParts(locality: "Reykjavík", administrativeArea: nil, country: nil),
    ]
    var asked: [[Double]] = []
    let names = Geocoder.names(for: [[64.15, -21.94], [34.69, 135.50], [35.01, 135.77]]) { lat, lon in
        asked.append([lat, lon])
        return places[lat]
    }
    #expect(names == ["Reykjavík", nil, "Kyoto"])
    #expect(asked == [[64.15, -21.94], [34.69, 135.50], [35.01, 135.77]])
}

@Test func geocodeNamesNeverAskForAnImpossibleCoordinate() {
    var asked = 0
    let names = Geocoder.names(for: [[91, 0], [1], [10, 200], [35.01, 135.77]]) { _, _ in
        asked += 1
        return PlaceParts(locality: "Kyoto", administrativeArea: nil, country: nil)
    }
    #expect(names == [nil, nil, nil, "Kyoto"])
    #expect(asked == 1)
}

@Test func geocodeRequestDecodesItsCoordinates() throws {
    let json = #"{"id":"g1","kind":"geocode","coordinates":[[35.01,135.77],[64.15,-21.94]]}"#
    let req = try JSONDecoder().decode(Request.self, from: Data(json.utf8))
    #expect(req.kind == .geocode)
    #expect(req.coordinates == [[35.01, 135.77], [64.15, -21.94]])
}

/// Rust reads a missing name as JSON null, one per coordinate. A nil that
/// dropped out of the array would shift every later name onto the wrong
/// chapter.
@Test func geocodeResponseEncodesAMissingNameAsNull() throws {
    let res = Response(id: "g1", result: .geocoded(["Kyoto", nil, "Reykjavík"]))
    let text = String(decoding: try JSONEncoder().encode(res), as: UTF8.self)
    #expect(text.contains(#""type":"geocoded""#))
    #expect(text.contains(#""data":["Kyoto",null,"Reykjavík"]"#))
}

@Test func geocodeRequestWithoutCoordinatesIsAnError() {
    let response = handle(line: #"{"id":"g2","kind":"geocode"}"#)
    #expect(response.id == "g2")
    guard case .error = response.result else {
        Issue.record("expected an error result, got \(response.result)")
        return
    }
}
