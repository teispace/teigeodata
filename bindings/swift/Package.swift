// swift-tools-version:6.0
// The Swift binding of teigeodata over its C ABI. Build the ABI first:
//   cargo build --release -p teigeodata-ffi
import PackageDescription

let package = Package(
    name: "TeiGeodata",
    products: [.library(name: "TeiGeodata", targets: ["TeiGeodata"])],
    targets: [
        .systemLibrary(name: "CTeiGeodata"),
        .target(
            name: "TeiGeodata",
            dependencies: ["CTeiGeodata"],
            linkerSettings: [.unsafeFlags(["-L../../target/release"])]
        ),
        .executableTarget(
            name: "Parity",
            dependencies: ["TeiGeodata"],
            linkerSettings: [.unsafeFlags(["-L../../target/release", "-Xlinker", "-rpath", "-Xlinker", "../../target/release"])]
        ),
    ]
)
