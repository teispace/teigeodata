// The prebuilt libraries of this package's release (its pubspec version), by Rust target
// triple. Written by the release pipeline (`cargo xtask release-pin`) from the files it
// uploads; empty in a checkout between releases, where the hook builds from source.

/// SHA-256 of each target's library file, lowercase hex.
const prebuiltSha256 = <String, String>{
  'aarch64-apple-darwin': 'f5ad93c7b5f6730896c5eacb2b65c793ddb601c8e7bc049ae45128f495322037',
  'aarch64-apple-ios': 'aacd1639aa0d8b3c62b980ee7ac93e82890310c06f916c0e4cccc80ef28d3f31',
  'aarch64-apple-ios-sim': 'a45a076481b3fe5ce766bd454666be9a480f4e68ae0455aa25f14a12beea7806',
  'aarch64-linux-android': 'ca5563e1014c8c3a274636a76ad945b07a01a220ae991e86ea9f0e32852e2837',
  'aarch64-pc-windows-msvc': '6df883025b00afdf2ab565092d4bfaba276eb78a65ecf0163da722d05a662026',
  'aarch64-unknown-linux-gnu': 'cd32850a17a6a3f2b9290caaabb2df99e9e57df1efc62a11a5e5757f19fb15e9',
  'aarch64-unknown-linux-musl': 'd5196838f55a53c503d0853d4a30f1858600088d19ac2929a65055a273235a27',
  'armv7-linux-androideabi': '4894534908be7ff483848b657a2e0529c5808df0ff1688162a17c05c69e5ede9',
  'x86_64-apple-darwin': 'cd1cb615648b9ac43e99fbcbcb887f1d20ee46ed4c611faddafd96663b29f09a',
  'x86_64-apple-ios': '751cc604c073e99f0257964c5b39f36ac540e74109673f06d0351317b321ff80',
  'x86_64-linux-android': 'f10fbf64eecbab9c3c25f32edbcd66fd707911d1405424a4ad45dfb800facb4a',
  'x86_64-pc-windows-msvc': 'e92ecf66c28413cee104b0e742a07d48b390a6cc93d9bdddea46799c091ca0c7',
  'x86_64-unknown-linux-gnu': '2564e9d07c191de04562f511a3dab3402b3d442a5df11e904d7a9b73fc6b0881',
  'x86_64-unknown-linux-musl': '487fcec8b24e6cf615151810b8511f29b6c23646da349684b5e0a30f733a6654',
};
