// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "NetCheck",
    platforms: [.macOS(.v13)],
    // Explicit dependency (rather than relying on the toolchain-bundled copy)
    // so `swift test` works with just the Command Line Tools, no full Xcode.
    dependencies: [
        .package(url: "https://github.com/apple/swift-testing.git", from: "0.12.0")
    ],
    targets: [
        .executableTarget(
            name: "NetCheckApp",
            path: "Sources/NetCheckApp"
        ),
        .testTarget(
            name: "NetCheckAppTests",
            dependencies: [
                "NetCheckApp",
                .product(name: "Testing", package: "swift-testing")
            ],
            path: "Tests/NetCheckAppTests"
        )
    ]
)
