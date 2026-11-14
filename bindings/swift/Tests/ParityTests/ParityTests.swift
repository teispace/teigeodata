// The parity checks as a test, so they run where no executable can: on an iOS simulator
// (cargo xtask package-swift --test). The simulator reads the repository's files in place.
import Foundation
import ParityChecks
import Testing

@Test func parity() async throws {
    // bindings/swift/Tests/ParityTests/ParityTests.swift, four levels below bindings/.
    var bindings = URL(fileURLWithPath: #filePath)
    for _ in 0..<4 { bindings.deleteLastPathComponent() }
    try await runParity(bindings.appendingPathComponent("parity.json").path)
}
