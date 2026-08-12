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
