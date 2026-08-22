// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "NetCheck",
    platforms: [.macOS(.v13)],
    targets: [
        .executableTarget(
            name: "NetCheckApp",
            path: "Sources/NetCheckApp"
        )
    ]
)
