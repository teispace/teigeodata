// swift-tools-version:6.0
// The Swift binding of teigeodata as released (D-287, D-304): its C library is the
// XCFramework of the release this commit is tagged with. Written by the release pipeline
// (`cargo xtask release-pin`); to work on the binding, use bindings/swift/Package.swift.
import PackageDescription

let package = Package(
    name: "TeiGeodata",
    platforms: [.iOS(.v15), .macOS(.v12)],
    products: [.library(name: "TeiGeodata", targets: ["TeiGeodata"])],
    targets: [
        .binaryTarget(
            name: "CTeiGeodata",
            url: "https://github.com/teispace/teigeodata/releases/download/v1.3.0/CTeiGeodata.xcframework.zip",
            checksum: "46c7b28f4f3e540d31335a722687ab5c087160612cd23010879fd6a7475fc600"
        ),
        .target(
            name: "TeiGeodata",
            dependencies: ["CTeiGeodata"],
            path: "bindings/swift/Sources/TeiGeodata",
            resources: [.copy("PrivacyInfo.xcprivacy")]
        ),
    ]
)
