// swift-tools-version:6.0
// The tour: every query of the guide, run on a released pack.
//
// Usage: swift run -c release Tour world-np-lite.tgd (in this directory)
import PackageDescription

let package = Package(
    name: "Tour",
    platforms: [.macOS(.v12)],
    dependencies: [
        // This checkout's package; an application names the released one (see the Swift guide).
        .package(path: "../../../../bindings/swift"),
    ],
    targets: [
        .executableTarget(
            name: "Tour",
            dependencies: [.product(name: "TeiGeodata", package: "swift")],
            // The C library this checkout builds (cargo build --release -p teigeodata-ffi).
            linkerSettings: [.unsafeFlags([
                "-L../../../../target/release",
                "-Xlinker", "-rpath", "-Xlinker", "../../../../target/release",
            ])]
        ),
    ]
)
