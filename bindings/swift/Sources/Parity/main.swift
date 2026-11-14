// Parity: the Swift binding gives the results bindings/parity.json expects (checked there
// against Rust). Run: swift run -c release Parity ../parity.json
import ParityChecks

try await runParity(CommandLine.arguments.dropFirst().first ?? "../parity.json")
print("swift parity ok")
