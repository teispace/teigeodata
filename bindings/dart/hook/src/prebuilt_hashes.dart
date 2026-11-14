// The prebuilt libraries of this package's release (its pubspec version), by Rust target
// triple. Written by the release pipeline when it uploads them (phase 5.10); empty until the
// first such release, so until then the hook builds from source.

/// SHA-256 of each target's library file, lowercase hex.
const prebuiltSha256 = <String, String>{};
