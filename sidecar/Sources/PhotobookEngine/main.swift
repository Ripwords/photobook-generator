import Foundation

while let line = readLine(strippingNewline: true) {
    if line.isEmpty { continue }
    emit(handle(line: line))
}
