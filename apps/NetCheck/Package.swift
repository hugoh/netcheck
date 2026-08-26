// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "NetCheck",
    platforms: [.macOS(.v13)],
    dependencies: [
        .package(url: "https://github.com/apple/swift-argument-parser.git", from: "1.5.0"),
        // Explicit dependency (rather than the toolchain-bundled copy) so
        // `swift test` works with just the Command Line Tools, no full Xcode.
        .package(url: "https://github.com/apple/swift-testing.git", from: "0.12.0"),
    ],
    targets: [
        // The native network-probing engine: interface/route/DNS/proxy/VPN/
        // Wi-Fi inspection plus reachability, resolution and ICMP probes.
        .target(
            name: "NetStatus",
            path: "Sources/NetStatus"
        ),
        .executableTarget(
            name: "NetCheckApp",
            dependencies: ["NetStatus"],
            path: "Sources/NetCheckApp"
        ),
        // Snapshot-only JSON CLI — one probe call per subcommand.
        .executableTarget(
            name: "netcheck",
            dependencies: [
                "NetStatus",
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
            ],
            path: "Sources/netcheck"
        ),
        .testTarget(
            name: "NetStatusTests",
            dependencies: ["NetStatus", .product(name: "Testing", package: "swift-testing")],
            path: "Tests/NetStatusTests"
        ),
        .testTarget(
            name: "NetCheckAppTests",
            dependencies: ["NetCheckApp", .product(name: "Testing", package: "swift-testing")],
            path: "Tests/NetCheckAppTests"
        ),
    ]
)
