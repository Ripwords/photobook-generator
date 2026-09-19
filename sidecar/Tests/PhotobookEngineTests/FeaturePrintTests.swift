import Testing
import Foundation
@testable import PhotobookEngine

// The literal is pinned byte-for-byte on the Rust side too
// (`cull::tests::decodes_the_feature_print_swift_pins`): 1.0 is 00 00 80 3F and
// -2.5 is 00 00 20 C0 as little-endian IEEE-754 Float32.
@Test func featurePrintEncodesFloat32LittleEndianAsBase64() {
    #expect(FeaturePrint.encode([1.0, -2.5]) == "AACAPwAAIMA=")
}

@Test func featurePrintDecodesWhatItEncodes() {
    let values: [Float] = [0.125, -0.5, 3.0e-7, 1.0]
    #expect(FeaturePrint.decode(FeaturePrint.encode(values)) == values)
}

@Test func featurePrintDecodeRefusesATruncatedElement() {
    // Six bytes: one whole Float32 and half of another.
    #expect(FeaturePrint.decode(Data([0, 0, 0x80, 0x3F, 0, 0]).base64EncodedString()) == nil)
}

@Test func featurePrintDecodeRefusesNonBase64() {
    #expect(FeaturePrint.decode("not base64!") == nil)
}
