import Foundation
import Vision

/// Wire form of a Vision image feature print: its Float32 elements,
/// little-endian, base64-encoded. Rust decodes the same bytes in
/// `book::cull::decode_feature_print`.
enum FeaturePrint {
    static func encode(_ values: [Float]) -> String {
        var data = Data(capacity: values.count * 4)
        for v in values {
            withUnsafeBytes(of: v.bitPattern.littleEndian) { data.append(contentsOf: $0) }
        }
        return data.base64EncodedString()
    }

    static func decode(_ base64: String) -> [Float]? {
        guard let data = Data(base64Encoded: base64), data.count % 4 == 0 else { return nil }
        return stride(from: 0, to: data.count, by: 4).map { i in
            let bits = data[data.startIndex + i..<data.startIndex + i + 4]
                .enumerated()
                .reduce(UInt32(0)) { $0 | UInt32($1.element) << (8 * UInt32($1.offset)) }
            return Float(bitPattern: bits)
        }
    }

    /// The observation's elements as Float32, whatever element type Vision
    /// stored them in.
    static func elements(of observation: VNFeaturePrintObservation) -> [Float] {
        observation.data.withUnsafeBytes { raw in
            switch observation.elementType {
            case .float: Array(raw.bindMemory(to: Float.self))
            case .double: raw.bindMemory(to: Double.self).map(Float.init)
            default: []
            }
        }
    }
}
