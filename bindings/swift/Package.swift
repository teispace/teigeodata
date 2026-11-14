// swift-tools-version:6.0
// The Swift binding of teigeodata over its C ABI.
//
// Two ways to get the C library:
// - development (default, macOS and Linux): the library built in the repository's target/,
//   `cargo build --release -p teigeodata-ffi` first;
// - the XCFramework (`TEIGEODATA_XCFRAMEWORK=1`): CTeiGeodata.xcframework beside this file,
//   made by `cargo xtask package-swift` for iOS, the iOS simulators and macOS, as the released
//   package ships it (D-287).
import PackageDescription

let xcframework = Context.environment["TEIGEODATA_XCFRAMEWORK"] == "1"
let devLink: [LinkerSetting] = xcframework ? [] : [.unsafeFlags(["-L../../target/release"])]
let devRun: [LinkerSetting] = xcframework ? [] : [
    .unsafeFlags(["-L../../target/release", "-Xlinker", "-rpath", "-Xlinker", "../../target/release"])
]

let package = Package(
    name: "TeiGeodata",
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [.library(name: "TeiGeodata", targets: ["TeiGeodata"])],
    targets: [
        xcframework
            ? .binaryTarget(name: "CTeiGeodata", path: "CTeiGeodata.xcframework")
            : .systemLibrary(name: "CTeiGeodata"),
        .target(
            name: "TeiGeodata",
            dependencies: ["CTeiGeodata"],
            // Apple's privacy manifest: the file metadata the library reads (D-287).
            resources: [.copy("PrivacyInfo.xcprivacy")],
            linkerSettings: devLink
        ),
        .target(name: "ParityChecks", dependencies: ["TeiGeodata"]),
        .executableTarget(name: "Parity", dependencies: ["ParityChecks"], linkerSettings: devRun),
        .executableTarget(name: "Bench", dependencies: ["TeiGeodata"], linkerSettings: devRun),
        .testTarget(name: "ParityTests", dependencies: ["ParityChecks"], linkerSettings: devRun),
    ]
)
